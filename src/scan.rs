// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use std::fmt;
use std::sync::LazyLock;
use std::time::Duration;

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use http::{HeaderMap, HeaderValue, header};
use memchr::memmem;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::domain::normalize_domain;
use crate::gostr::{go_quote, go_string, to_lower};
use crate::matching::SIGNATURE_INDEX;
use crate::netguard::{self, Client, Fetched};
use crate::signatures::{COMPILED, CmsScore, select_winners};

const PRIMARY_FETCH_TIMEOUT: Duration = Duration::from_secs(8);
const BYPASS_FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const SCRAPER_FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// A site must not be able to exhaust the scanner's memory with its response.
const MAX_SCAN_BYTES: u64 = 5 << 20;
const MAX_REDIRECTS: usize = 5;

const DEFAULT_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/120.0.0.0";

/// Tunes `scan`. The default works: no third party is contacted, and pages are
/// requested in English.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Enables a retry through ScraperAPI (scraperapi.com) when a site blocks
    /// the direct request or the scanner's address. Empty means the scan never
    /// leaves this machine.
    pub scraper_api_key: String,
    /// ScraperAPI's `country_code`, such as "us" or "de": some sites answer
    /// only visitors from their own country. Empty leaves the choice to
    /// ScraperAPI.
    pub scraper_country: String,
    /// Sent with every request; empty means "en-US,en;q=0.9".
    pub accept_language: String,
}

fn scan_headers(opts: &Options) -> Result<HeaderMap, Error> {
    let lang = first_non_empty(&opts.accept_language, DEFAULT_ACCEPT_LANGUAGE);
    let lang = HeaderValue::from_str(lang)
        .map_err(|_| Error::Fetch(format!("invalid Accept-Language {}", go_quote(lang))))?;
    let mut h = HeaderMap::new();
    h.insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
    h.insert(
        header::ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
    );
    h.insert(header::ACCEPT_LANGUAGE, lang);
    Ok(h)
}

/// Looks for text markers of anti-bot challenge pages. The check is heuristic
/// (a challenge page does not identify itself with a header), so it is a set
/// of strings rather than a structural test.
fn is_waf_page(body: &[u8]) -> bool {
    let has = |needle: &str| memmem::find(body, needle.as_bytes()).is_some();
    if has("rt-solar.ru")
        || has("Подтвердите, что&nbsp;вы человек")
        || has("Подтвердите что вы не робот")
        || has("__cf_chl_opt")
        || has("cf-challenge")
        || has("Checking your browser before accessing")
    {
        return true;
    }
    has("ddos-guard") && body.len() < 20000
}

/// The outcome of a scan. `Failed` is never returned by `scan`: the caller
/// sets it when neither https nor http could be opened (see `scan`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The page was read.
    Ok,
    /// An anti-bot page answered, and the bypass did not get past it.
    Cloudflare,
    /// Set by the caller, see the type.
    Failed,
}

impl Status {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Cloudflare => "cloudflare",
            Self::Failed => "failed",
        }
    }
}

/// Describes the response the detection was made from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Details {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    pub server: String,
    #[serde(rename = "statusCode")]
    pub status_code: u16,
}

/// What `scan` found. `detected` holds at most one backend engine and at most
/// one presentation layer on top of it, such as `["Next.js", "WordPress"]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    pub status: Status,
    pub detected: Vec<String>,
    /// The answer came through the WAF bypass.
    #[serde(rename = "wafBehind")]
    pub waf_behind: bool,
    pub details: Details,
}

/// Why `scan` gave no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input is not a host name `scan` will fetch: an IP address, a name
    /// without a dot, or something that is not a URL at all.
    InvalidDomain(String),
    /// The name is not in DNS. It separates a typo in the address from a site
    /// we could not reach. The difference matters: there is nobody to tell
    /// about a name that does not exist, while the owner of a live name that
    /// does not answer needs to know.
    NoSuchHost(String),
    /// Neither https nor http could be opened, and the bypass did not help.
    Fetch(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDomain(input) => write!(f, "not a domain name: {}", go_quote(input)),
            Self::NoSuchHost(domain) => write!(f, "no such host: {domain}"),
            Self::Fetch(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for Error {}

static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<title>([^<]+)</title>").expect("a fixed expression"));

fn first_non_empty<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.is_empty() { b } else { a }
}

/// Tries https and falls back to http. Address selection, redirects and the
/// ban on internal networks all live in the netguard client.
async fn perform_initial_fetch(
    client: &Client,
    domain: &str,
) -> Result<(String, Fetched), netguard::Error> {
    let secure = format!("https://{domain}");
    if let Ok(res) = client.fetch(&secure).await {
        return Ok((secure, res));
    }
    let plain = format!("http://{domain}");
    let res = client.fetch(&plain).await?;
    Ok((plain, res))
}

struct BypassPair {
    direct: Client,
    scraper: Client,
}

fn bypass_clients(headers: &HeaderMap) -> BypassPair {
    BypassPair {
        direct: netguard::new(&netguard::Options {
            timeout: BYPASS_FETCH_TIMEOUT,
            max_bytes: MAX_SCAN_BYTES,
            max_redirects: MAX_REDIRECTS,
            header: headers.clone(),
            insecure_tls: true,
        }),
        scraper: netguard::new(&netguard::Options {
            timeout: SCRAPER_FETCH_TIMEOUT,
            max_bytes: MAX_SCAN_BYTES,
            max_redirects: MAX_REDIRECTS,
            header: headers.clone(),
            insecure_tls: false,
        }),
    }
}

/// Tries ScraperAPI (when a key is set) and a direct request with certificate
/// checks off at the same time; the first success wins, so a quick failure of
/// one attempt does not bury the other. The attempt that loses is dropped,
/// which cancels it.
async fn attempt_waf_bypass(
    clients: &BypassPair,
    target_url: &str,
    opts: &Options,
) -> Option<Fetched> {
    let try_fetch = |client: &Client, u: String| {
        let client = client.clone();
        async move {
            match client.fetch(&u).await {
                Ok(res) if res.status_code == 200 && !is_waf_page(&res.body) => Some(res),
                _ => None,
            }
        }
    };

    let mut attempts = FuturesUnordered::new();
    if !opts.scraper_api_key.is_empty() {
        // No `render=true`: the response is byte for byte the same but costs one
        // credit instead of ten. Signatures read the server's markup, and the direct
        // path does not execute it either; rendering the page would mean checking
        // something other than what the main fetcher sees
        let mut scraper_url = format!(
            "http://api.scraperapi.com?api_key={}&url={}",
            query_escape(&opts.scraper_api_key),
            query_escape(target_url)
        );
        if !opts.scraper_country.is_empty() {
            scraper_url.push_str("&country_code=");
            scraper_url.push_str(&query_escape(&opts.scraper_country));
        }
        attempts.push(try_fetch(&clients.scraper, scraper_url));
    }
    attempts.push(try_fetch(&clients.direct, target_url.to_owned()));

    while let Some(res) = attempts.next().await {
        if res.is_some() {
            return res;
        }
    }
    None
}

/// `url.QueryEscape` of Go: a space is `+`, `~` stays, the rest of what is not
/// unreserved is %-escaped.
fn query_escape(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(b));
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(char::from(HEX[usize::from(b >> 4)]));
                out.push(char::from(HEX[usize::from(b & 15)]));
            }
        }
    }
    out
}

/// Fetches the site and detects what it runs. The input may be a URL or a bare
/// host; it goes through `normalize_domain` first. Everything network-related
/// goes through the netguard client, which checks the address each socket
/// actually opens to, on every redirect hop and on the WAF bypass too, so a
/// redirect cannot lead the scanner into an internal network.
///
/// There is no time limit of its own beyond the limits of each request: wrap
/// the call in `tokio::time::timeout` for one.
///
/// # Errors
///
/// `InvalidDomain` before any request, `NoSuchHost` when the name does not
/// resolve, `Fetch` when the site could not be read and the bypass did not
/// help either.
pub async fn scan(input: &str, opts: &Options) -> Result<ScanResult, Error> {
    let Some(domain) = normalize_domain(input) else {
        return Err(Error::InvalidDomain(input.to_owned()));
    };
    let headers = scan_headers(opts)?;
    let primary = netguard::new(&netguard::Options {
        timeout: PRIMARY_FETCH_TIMEOUT,
        max_bytes: MAX_SCAN_BYTES,
        max_redirects: MAX_REDIRECTS,
        header: headers.clone(),
        insecure_tls: false,
    });

    let (target_url, mut page, fetch_err) = match perform_initial_fetch(&primary, &domain).await {
        Ok((url, res)) => (url, res, None),
        // The name is not in DNS: there is nothing to open, and the bypass would
        // answer the same. The check sits here so that a typo in the domain does not
        // cost the user another twenty seconds of waiting
        Err(netguard::Error::Dns(_)) => return Err(Error::NoSuchHost(domain)),
        Err(e) => (format!("https://{domain}"), Fetched::default(), Some(e)),
    };
    // The Server header is taken from the direct response only: the bypass has its
    // own, and ScraperAPI's header must not be passed off as the site's
    let server_header = page
        .header
        .get(header::SERVER)
        .map(|v| to_lower(&go_string(v.as_bytes())))
        .unwrap_or_default();

    let mut waf_behind = false;
    if needs_bypass(&page, fetch_err.as_ref(), opts) {
        let Some(bypassed) = attempt_waf_bypass(&bypass_clients(&headers), &target_url, opts).await
        else {
            if let Some(e) = fetch_err {
                return Err(Error::Fetch(e.to_string()));
            }
            return Ok(ScanResult {
                status: Status::Cloudflare,
                detected: Vec::new(),
                waf_behind: false,
                details: Details {
                    title: String::new(),
                    server: server_header,
                    status_code: page.status_code,
                },
            });
        };
        page = bypassed;
        waf_behind = true;
    }

    let html = go_string(&page.body);
    let title = TITLE_RE
        .captures(&html)
        .map(|m| m[1].trim().to_owned())
        .unwrap_or_default();

    Ok(ScanResult {
        status: Status::Ok,
        detected: detect_page(&page.body, &html, &page.header),
        waf_behind,
        details: Details {
            title,
            server: first_non_empty(&server_header, "unknown").to_owned(),
            status_code: page.status_code,
        },
    })
}

/// Decides whether the direct answer is worth a second attempt.
fn needs_bypass(page: &Fetched, fetch_err: Option<&netguard::Error>, opts: &Options) -> bool {
    if fetch_err.is_some() {
        // The direct request did not happen at all, but the site may be alive and
        // simply refuse our address: anti-scraping filters block hosting ranges, and
        // some sites do not even answer a SYN from a datacenter. Only a different
        // address helps, so without a key the bypass is not started: a local retry
        // differs from the failed attempt only by the certificate check, and that is
        // not why the connection failed
        return !opts.scraper_api_key.is_empty();
    }
    page.status_code == 403 || page.status_code == 503 || is_waf_page(&page.body)
}

/// Names the engine (and the presentation layer on top of it, if any) from a
/// page body and its response headers, without any network access. It returns
/// an empty list when nothing scores high enough.
#[must_use]
pub fn detect(body: &[u8], headers: &HeaderMap) -> Vec<String> {
    detect_page(body, &go_string(body), headers)
}

fn detect_page(raw: &[u8], html: &str, headers: &HeaderMap) -> Vec<String> {
    select_winners(&score_response(raw, html, headers))
}

/// Scores in table order: `select_winners` breaks ties by it. Headers are
/// matched by their expressions directly, since they are short. `raw` and
/// `html` are the same body: the automaton reads bytes, the expressions text.
pub(crate) fn score_response(raw: &[u8], html: &str, headers: &HeaderMap) -> Vec<CmsScore> {
    let index = &*SIGNATURE_INDEX;
    let hits = index.scan(raw);
    let mut out = Vec::new();
    for (i, entry) in COMPILED.iter().enumerate() {
        let mut score = 0.0;
        for (j, f) in entry.body.iter().enumerate() {
            if index.plans[i][j].matches(&hits, &f.re, html) {
                score += f.weight;
            }
        }
        for (name, items) in &entry.headers {
            let vals: Vec<String> = headers
                .get_all(*name)
                .iter()
                .map(|v| go_string(v.as_bytes()).into_owned())
                .collect();
            if vals.is_empty() {
                continue;
            }
            let val = vals.join(", ");
            for f in items {
                if f.re.is_match(&val) {
                    score += f.weight;
                }
            }
        }
        if score > 0.0 {
            out.push(CmsScore {
                name: entry.name.to_owned(),
                score,
            });
        }
    }
    out
}
