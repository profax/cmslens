// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use std::sync::LazyLock;

use regex::Regex;

use crate::gostr::{go_string, to_lower};
use crate::netguard::parse_addr;
use crate::punycode;

/// Checks the ASCII form of the host: a name typed in Cyrillic reaches it as
/// punycode, so the TLD may look like xn--p1ai (.рф).
static HOSTNAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z0-9](?:[a-z0-9._-]*[a-z0-9])?\.(?:xn--[a-z0-9-]+|[a-z]{2,})$")
        .expect("a fixed expression")
});

/// Turns user input (a URL, a bare host, with or without www) into a host name
/// to scan. A name in Cyrillic or another script comes back as punycode:
/// президент.рф becomes xn--d1abbgf6aiiy.xn--p1ai. IP literals are rejected:
/// there is no reason to scan by address bypassing DNS, and an address is the
/// easiest way to reach an internal network (the fetcher also checks the
/// address it actually connects to, not just the name typed in).
#[must_use]
pub fn normalize_domain(input: &str) -> Option<String> {
    let s = to_lower(input.trim());
    if s.is_empty() {
        return None;
    }
    let s = s.strip_prefix("https://").unwrap_or(&s);
    let s = s.strip_prefix("http://").unwrap_or(s);
    let s = s.strip_prefix("www.").unwrap_or(s);
    let s = s.split('/').next().unwrap_or(s);
    let s = s.split('?').next().unwrap_or(s);
    let s = s.split('#').next().unwrap_or(s);

    let host = hostname(s)?;
    if host.is_empty() || host.starts_with('[') {
        return None;
    }
    // The host comes as typed; DNS and the checks below need ASCII
    let host = punycode::to_ascii(&host).ok()?;
    if parse_addr(&host).is_some() {
        return None;
    }
    HOSTNAME_RE.is_match(&host).then_some(host)
}

/// `url.Parse("http://" + authority).Hostname()` of Go, for an authority with
/// no path, query or fragment left: the user part is checked and dropped, the
/// port is checked and dropped, %-escapes are decoded where Go allows them in
/// a host (bytes outside ASCII and `%25` only).
fn hostname(authority: &str) -> Option<String> {
    // Go refuses a URL with a control character anywhere in it
    if authority.bytes().any(|b| b < b' ' || b == 0x7f) {
        return None;
    }
    let (user, host) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    let host = parse_host(host)?;
    if let Some(user) = user {
        if !valid_userinfo(user) {
            return None;
        }
        for part in user.splitn(2, ':') {
            unescape(part, false)?;
        }
    }
    // Hostname() drops a valid port
    let host = match host.rfind(':') {
        Some(i) if valid_optional_port(&host[i..]) => &host[..i],
        _ => &host[..],
    };
    Some(host.to_owned())
}

fn parse_host(host: &str) -> Option<String> {
    // Go accepts only an IPv6 address inside brackets, and an address is
    // rejected either way: there is nothing to scan behind it
    if host.starts_with('[') {
        return None;
    }
    if let Some(i) = host.rfind(':')
        && !valid_optional_port(&host[i..])
    {
        return None;
    }
    unescape(host, true)
}

/// `validOptionalPort`: empty, or a colon followed by digits only.
fn valid_optional_port(port: &str) -> bool {
    match port.strip_prefix(':') {
        None => port.is_empty(),
        Some(digits) => digits.bytes().all(|b| b.is_ascii_digit()),
    }
}

/// `validUserinfo`: RFC 3986 allows these characters, Go also lets `@` pass.
fn valid_userinfo(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._:~!$&'()*+,;=%@".contains(c))
}

/// `unescape` of net/url in host mode (`host` true) or in user mode. A host
/// may %-encode only bytes outside ASCII (and `%25` in a zone), and an ASCII
/// byte the host does not allow is an error rather than something to escape.
fn unescape(s: &str, host: bool) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let (hi, lo) = (b.get(i + 1).copied(), b.get(i + 2).copied());
                let (Some(hi), Some(lo)) = (hi.and_then(unhex), lo.and_then(unhex)) else {
                    return None;
                };
                if host && hi < 8 && &b[i..i + 3] != b"%25" {
                    return None;
                }
                out.push(hi << 4 | lo);
                i += 3;
            }
            c if host && c < 0x80 && should_escape_in_host(c) => return None,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Some(go_string(&out).into_owned())
}

fn unhex(c: u8) -> Option<u8> {
    char::from(c)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

/// `shouldEscape(c, encodeHost)`: what a host may hold unescaped.
fn should_escape_in_host(c: u8) -> bool {
    !(c.is_ascii_alphanumeric() || b"!$&'()*+,;=:[]<>\"-_.~".contains(&c))
}
