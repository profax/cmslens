// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! The only way out to the network for code that fetches an address supplied
//! by a user.
//!
//! What is checked is neither the host name nor the result of resolving it,
//! but the address the socket is about to connect to. The difference is not
//! theoretical: between "resolved and allowed" and "connected" there is room
//! for a second DNS answer pointing to 127.0.0.1, and a check by name does not
//! see that swap. The ban lives in the client's resolver: reqwest connects only
//! to the addresses it hands out, and it hands out no closed ones, so the ban
//! fires on every attempt: on the host's fallback addresses and on every
//! redirect hop, without a single line in the calling code. An address
//! written as digits in the URL is not resolved by reqwest at all, so
//! `safe_url` checks it at the entrance and the redirect policy on every hop.
//!
//! This also explains what the module does NOT do. It does not resolve the
//! host itself and reject it whole when any record points inside: a host with
//! two addresses, one of them internal, stays reachable via the external one,
//! and a connection inward simply does not open.
//!
//! The stakes, stated plainly: <http://169.254.169.254/> hands out cloud
//! credentials, and services on 127.0.0.1 are a database or an admin panel.

use std::error::Error as StdError;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use futures::{Stream, StreamExt};
use http::HeaderMap;
use reqwest::Url;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

use crate::gostr::go_quote;

/// Why a fetch failed. `Blocked` is the one signal by which a caller tells
/// "not allowed there" from "the network did not answer"; `Dns` tells a name
/// that could not be resolved from a site that did not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Error {
    Blocked(String),
    Dns(String),
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blocked(text) | Self::Dns(text) | Self::Other(text) => f.write_str(text),
        }
    }
}

impl StdError for Error {}

fn blocked_target(what: impl Into<String>) -> Error {
    Error::Blocked(format!("blocked target: {}", what.into()))
}

/// Where the fetcher may not go. The list is explicit rather than "everything
/// that is not public": the ready-made predicates of `std::net` know private
/// networks and loopback, but not 169.254.169.254 (cloud metadata),
/// 100.64.0.0/10 (neighbours at the same provider) or `2002::/16` (the same
/// 127.0.0.1 written as IPv6).
const BLOCKED_V4: &[(Ipv4Addr, u32)] = &[
    (Ipv4Addr::UNSPECIFIED, 8), // "this host", behaves as localhost on Linux
    (Ipv4Addr::new(10, 0, 0, 0), 8), // private network
    (Ipv4Addr::new(100, 64, 0, 0), 10), // CGNAT, addresses of neighbours at the same provider
    (Ipv4Addr::new(127, 0, 0, 0), 8), // all of loopback, not just 127.0.0.1
    (Ipv4Addr::new(169, 254, 0, 0), 16), // link-local, cloud metadata lives here
    (Ipv4Addr::new(172, 16, 0, 0), 12), // private network
    (Ipv4Addr::new(192, 0, 0, 0), 24), // IETF protocol assignments
    (Ipv4Addr::new(192, 0, 2, 0), 24), // documentation
    (Ipv4Addr::new(192, 88, 99, 0), 24), // 6to4 relay
    (Ipv4Addr::new(192, 168, 0, 0), 16), // private network
    (Ipv4Addr::new(198, 18, 0, 0), 15), // network benchmark testing
    (Ipv4Addr::new(198, 51, 100, 0), 24), // documentation
    (Ipv4Addr::new(203, 0, 113, 0), 24), // documentation
    (Ipv4Addr::new(224, 0, 0, 0), 3), // multicast and everything reserved above it, broadcast included
];

const BLOCKED_V6: &[(Ipv6Addr, u32)] = &[
    (Ipv6Addr::UNSPECIFIED, 96), // covers ::, ::1 and the deprecated form ::127.0.0.1
    (Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0), 96), // NAT64, the address carries IPv4 inside
    (Ipv6Addr::new(0x64, 0xff9b, 1, 0, 0, 0, 0, 0), 48), // the same, local-use variant
    (Ipv6Addr::new(0x100, 0, 0, 0, 0, 0, 0, 0), 64), // discard prefix
    (Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23), // IETF protocol assignments, Teredo included
    (Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0), 32), // documentation
    (Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16), // 6to4, IPv4 inside the address too
    (Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7), // unique local addresses, fc and fd
    (Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10), // link-local
    (Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0), 8), // multicast
];

/// Answers a single question: may a connection be opened to this address.
pub(crate) fn is_blocked(addr: IpAddr) -> bool {
    // `IpAddr` has no zone, so a zone cannot carry the address past the list.
    // `to_canonical` turns ::ffff:127.0.0.1 back into IPv4; otherwise it slips
    // past the list, since an IPv4 prefix holds no IPv6 address.
    match addr.to_canonical() {
        IpAddr::V4(a) => {
            let a = a.to_bits();
            BLOCKED_V4
                .iter()
                .any(|&(net, len)| a >> (32 - len) == net.to_bits() >> (32 - len))
        }
        IpAddr::V6(a) => {
            let a = a.to_bits();
            BLOCKED_V6
                .iter()
                .any(|&(net, len)| a >> (128 - len) == net.to_bits() >> (128 - len))
        }
    }
}

/// `netip.ParseAddr`: an IPv6 address may carry a zone (`fe80::1%eth0`), which
/// `IpAddr` does not keep.
pub(crate) fn parse_addr(s: &str) -> Option<IpAddr> {
    if let Some((addr, zone)) = s.split_once('%') {
        if zone.is_empty() {
            return None;
        }
        return addr.parse::<Ipv6Addr>().ok().map(IpAddr::V6);
    }
    s.parse().ok()
}

/// Rejects what is visible from the name alone, spending neither a lookup nor
/// a connection. It is a convenience and a clear error at the entrance, not
/// the barrier: the barrier is the resolver, and nothing can be opened while
/// bypassing it.
pub(crate) fn check_host(host: &str) -> Result<(), Error> {
    let host = host.trim_matches(['[', ']']);
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() {
        return Err(blocked_target("empty host"));
    }
    if let Some(addr) = parse_addr(host) {
        if is_blocked(addr) {
            return Err(blocked_target(host));
        }
        return Ok(());
    }
    // The resolver completes a name without a dot with the local search suffix
    // and leads it into the machine's network rather than the internet.
    let lower = host.to_lowercase();
    if !lower.contains('.') || lower == "localhost" || lower.ends_with(".localhost") {
        return Err(blocked_target(host));
    }
    Ok(())
}

/// Parses an address and says whether it may be fetched at all.
pub(crate) fn safe_url(raw: &str) -> Result<Url, Error> {
    let parsed = match Url::parse(raw.trim()) {
        Ok(u) => u,
        // Go's url.Parse reads an address without a scheme as a path, and the
        // refusal is about the empty scheme
        Err(url::ParseError::RelativeUrlWithoutBase) => return Err(scheme_error("")),
        Err(_) => return Err(blocked_target("unparsable address")),
    };
    check_scheme(&parsed)?;
    check_host(parsed.host_str().unwrap_or(""))?;
    Ok(parsed)
}

fn check_scheme(target: &Url) -> Result<(), Error> {
    if target.scheme() != "http" && target.scheme() != "https" {
        return Err(scheme_error(target.scheme()));
    }
    Ok(())
}

fn scheme_error(scheme: &str) -> Error {
    blocked_target(format!("scheme {}", go_quote(scheme)))
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_BYTES: u64 = 2 << 20;
const DEFAULT_MAX_REDIRECTS: usize = 3;
const MAX_IDLE_CONNS: usize = 32;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// The limits of a client; a zero field takes its default.
#[derive(Debug, Clone, Default)]
pub(crate) struct Options {
    /// For the whole request, reading the body included.
    pub(crate) timeout: Duration,
    /// The body is cut here, and `Fetched::truncated` says so.
    pub(crate) max_bytes: u64,
    pub(crate) max_redirects: usize,
    pub(crate) header: HeaderMap,
    /// Turns off certificate verification of the target. The SSRF barrier in
    /// the resolver does not depend on it: this is a separate property
    /// (protection from MITM), not protection of the address. The CMS detector
    /// needs it to get past a WAF on sites with an expired or self-signed
    /// certificate, so the choice belongs to the caller rather than being a
    /// module-wide setting.
    pub(crate) insecure_tls: bool,
}

/// Fetches pages through the barrier; build it with `new`.
#[derive(Debug, Clone)]
pub(crate) struct Client {
    pub(crate) http: reqwest::Client,
    timeout: Duration,
    max_bytes: u64,
    header: HeaderMap,
}

/// One fetched page.
#[derive(Debug, Clone, Default)]
pub(crate) struct Fetched {
    /// After all redirects, not the one we started from.
    #[allow(dead_code)] // part of the answer as Go gives it; the scanner reads the rest
    pub(crate) url: String,
    pub(crate) status_code: u16,
    pub(crate) header: HeaderMap,
    /// Raw bytes: decoding from legacy charsets is up to the page parser.
    pub(crate) body: Vec<u8>,
    #[allow(dead_code)]
    pub(crate) truncated: bool,
}

/// Builds a client; every connection it opens goes through the resolver's
/// barrier.
///
/// # Panics
///
/// Only if reqwest cannot build a client from a complete TLS configuration.
pub(crate) fn new(opts: &Options) -> Client {
    let timeout = if opts.timeout.is_zero() {
        DEFAULT_TIMEOUT
    } else {
        opts.timeout
    };
    let max_bytes = if opts.max_bytes == 0 {
        DEFAULT_MAX_BYTES
    } else {
        opts.max_bytes
    };
    let max_redirects = if opts.max_redirects == 0 {
        DEFAULT_MAX_REDIRECTS
    } else {
        opts.max_redirects
    };

    let http = reqwest::Client::builder()
        // Proxies from the environment are off on purpose: with a proxy the
        // socket opens to the proxy, not to the target, and the resolver would
        // check the proxy's address. The ban on internal networks would cease to
        // exist, and silently.
        .no_proxy()
        .dns_resolver(GuardResolver)
        .tls_backend_preconfigured(tls_config(opts.insecure_tls))
        .connect_timeout(timeout)
        .tcp_keepalive(IDLE_TIMEOUT)
        .pool_max_idle_per_host(MAX_IDLE_CONNS)
        .pool_idle_timeout(IDLE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(
            move |attempt| match check_redirect(
                attempt.url(),
                attempt.previous().len(),
                max_redirects,
            ) {
                Ok(()) => attempt.follow(),
                Err(e) => attempt.error(e),
            },
        ))
        .build()
        .expect("a reqwest client with a complete TLS configuration");
    Client {
        http,
        timeout,
        max_bytes,
        header: opts.header.clone(),
    }
}

/// The redirect policy: `via` is the number of requests made before this hop.
pub(crate) fn check_redirect(next: &Url, via: usize, max_redirects: usize) -> Result<(), Error> {
    if via > max_redirects {
        return Err(blocked_target(format!(
            "more than {max_redirects} redirects"
        )));
    }
    // The resolver will check the address of the next hop by name, but nothing
    // else checks the scheme: Location comes from someone else's server, and a
    // file:// in it is their choice, not ours.
    check_scheme(next)?;
    // An address in digits is not resolved, reqwest connects to it directly:
    // its check sits here, on every hop
    let ip = match next.host() {
        Some(url::Host::Ipv4(a)) => IpAddr::V4(a),
        Some(url::Host::Ipv6(a)) => IpAddr::V6(a),
        _ => return Ok(()),
    };
    guard_dial(
        "tcp",
        &SocketAddr::new(ip, next.port_or_known_default().unwrap_or(0)).to_string(),
    )
}

/// Mozilla's roots instead of the system store: the program ships as one
/// static file, and the roots travel with it.
fn tls_config(insecure: bool) -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .expect("ring knows the default TLS versions");
    let mut config = if insecure {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerify(provider)))
            .with_no_client_auth()
    } else {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    // h2 is offered as Go's ForceAttemptHTTP2 offers it
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    config
}

/// No certificate chain check; the handshake signature is still verified with
/// the certificate's key, as Go's `InsecureSkipVerify` does.
#[derive(Debug)]
struct NoVerify(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// A name the resolver could not turn into addresses. It travels inside
/// reqwest's error as its source, so the caller can tell it apart.
#[derive(Debug)]
struct DnsError(String);

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for DnsError {}

/// The barrier: reqwest connects only to the addresses the resolver gave, and
/// there are no closed ones among them. A host without a single open address
/// gets the refusal of its first closed one.
struct GuardResolver;

impl Resolve for GuardResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let found = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| {
                    Box::new(DnsError(format!("lookup {host}: {e}")))
                        as Box<dyn StdError + Send + Sync>
                })?;
            let mut open = Vec::new();
            let mut refused = None;
            for addr in found {
                match guard_dial("tcp", &addr.to_string()) {
                    Ok(()) => open.push(addr),
                    Err(e) => {
                        refused.get_or_insert(e);
                    }
                }
            }
            if open.is_empty() {
                let err: Box<dyn StdError + Send + Sync> = match refused {
                    Some(e) => Box::new(e),
                    None => Box::new(DnsError(format!("lookup {host}: no such host"))),
                };
                return Err(err);
            }
            Ok(Box::new(open.into_iter()) as Addrs)
        })
    }
}

/// The check of the very address the socket is about to open to.
pub(crate) fn guard_dial(network: &str, address: &str) -> Result<(), Error> {
    if !network.starts_with("tcp") {
        return Err(blocked_target(format!("network {}", go_quote(network))));
    }
    let Some(host) = split_host(address) else {
        return Err(blocked_target(address));
    };
    let Some(addr) = parse_addr(host) else {
        return Err(blocked_target(address));
    };
    if is_blocked(addr) {
        return Err(blocked_target(addr.to_string()));
    }
    Ok(())
}

/// The host of `net.SplitHostPort`: `[v6]:port`, or `host:port` with one colon.
fn split_host(address: &str) -> Option<&str> {
    if let Some(rest) = address.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        tail.strip_prefix(':').filter(|p| !p.contains(':'))?;
        return Some(host);
    }
    let (host, _) = address.rsplit_once(':')?;
    (!host.contains(':') && !host.contains('[') && !host.contains(']')).then_some(host)
}

impl Client {
    /// Reads the whole page, but no more than the cap: a scanner must not let
    /// a gigabyte response eat its memory.
    pub(crate) async fn fetch(&self, raw_url: &str) -> Result<Fetched, Error> {
        let target = safe_url(raw_url)?;
        let shown = raw_url.trim().to_owned();
        let resp = self
            .http
            .get(target.clone())
            .timeout(self.timeout)
            .headers(self.header.clone())
            .send()
            .await
            .map_err(|e| request_error(&shown, &target, &e))?;
        let url = resp.url().to_string();
        let status_code = resp.status().as_u16();
        let header = resp.headers().clone();
        let (body, truncated) = read_capped(resp.bytes_stream(), self.max_bytes)
            .await
            .map_err(|e| Error::Other(format!("reading response body: {}", error_text(&e))))?;
        Ok(Fetched {
            url,
            status_code,
            header,
            body,
            truncated,
        })
    }
}

/// The barrier's verdict and the resolver's failure reach the caller through
/// the wrappers of reqwest and hyper as a source. The text is the one Go
/// showed (`Get "<address>": <cause>`), with the address of the hop that
/// failed: a refused address reads `dial tcp <address>: blocked target: …`,
/// as Go's dialer wrote it.
fn request_error(shown: &str, target: &Url, err: &reqwest::Error) -> Error {
    let failed = err.url().filter(|u| *u != target);
    let url = go_quote(&failed.map_or_else(|| shown.to_owned(), Url::to_string));
    let mut cur: Option<&(dyn StdError + 'static)> = Some(err);
    while let Some(e) = cur {
        if let Some(Error::Blocked(what)) = e.downcast_ref::<Error>() {
            let hop = failed.unwrap_or(target);
            let inner = what.strip_prefix("blocked target: ").unwrap_or(what);
            return Error::Blocked(match parse_addr(inner) {
                Some(ip) => {
                    let port = hop.port_or_known_default().unwrap_or(0);
                    format!("Get {url}: dial tcp {}: {what}", SocketAddr::new(ip, port))
                }
                None => format!("Get {url}: {what}"),
            });
        }
        if e.downcast_ref::<DnsError>().is_some() {
            return Error::Dns(format!("Get {url}: {}", error_text(err)));
        }
        cur = e.source();
    }
    Error::Other(format!("Get {url}: {}", error_text(err)))
}

/// The text of a network error: a timeout in Go's words, otherwise the chain
/// of sources.
fn error_text(err: &reqwest::Error) -> String {
    if err.is_timeout() {
        return "context deadline exceeded".to_owned();
    }
    let mut text = err.to_string();
    let mut cur = err.source();
    while let Some(e) = cur {
        let part = e.to_string();
        if !text.contains(&part) {
            text.push_str(": ");
            text.push_str(&part);
        }
        cur = e.source();
    }
    text
}

/// Reads one byte more than the cap; otherwise "exactly the cap" and
/// "truncated" cannot be told apart.
pub(crate) async fn read_capped<S, B, E>(source: S, max_bytes: u64) -> Result<(Vec<u8>, bool), E>
where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
{
    let cap = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let mut body = Vec::new();
    let mut source = std::pin::pin!(source);
    while let Some(chunk) = source.next().await {
        let chunk = chunk?;
        let chunk = chunk.as_ref();
        let room = cap.saturating_add(1) - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if body.len() > cap {
            body.truncate(cap);
            return Ok((body, true));
        }
    }
    Ok((body, false))
}
