// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use reqwest::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::netguard::{
    self, Error, check_host, check_redirect, guard_dial, is_blocked, parse_addr, read_capped,
    safe_url,
};

#[test]
fn closed_ranges_stay_closed() {
    let closed = [
        "127.0.0.1",
        "127.1.2.3",
        "0.0.0.0",
        "10.0.0.1",
        "192.168.1.10",
        "172.16.0.1",
        "172.31.255.254",
        "169.254.169.254", // cloud metadata, the reason all of this exists
        "100.64.0.1",      // CGNAT
        "224.0.0.1",       // multicast
        "255.255.255.255", // broadcast
        "192.0.0.1",
        "192.0.2.5",
        "198.18.0.1",
        "198.51.100.7",
        "203.0.113.9",
        "::",
        "::1",
        "fe80::1",
        "fd00::1",
        "fc00::1",
        "ff02::1",
        "::ffff:127.0.0.1", // the same loopback written IPv4-mapped
        "::127.0.0.1",      // and again in the deprecated IPv4-compatible form
        "2002:7f00:1::",    // 6to4: IPv4 embedded in the address
        "64:ff9b::7f00:1",  // NAT64: the same in another way
        "2001::1",          // Teredo
        "fe80::1%eth0",     // a zone must not carry the address past the list
    ];
    for raw in closed {
        let addr = parse_addr(raw).unwrap_or_else(|| panic!("{raw} did not parse as an address"));
        assert!(is_blocked(addr), "{raw} is open but must be closed");
    }
}

#[test]
fn public_addresses_pass() {
    for raw in [
        "8.8.8.8",
        "93.184.216.34",
        "172.32.0.1",
        "100.128.0.1",
        "2606:4700::1111",
        "2a00:1450:4010::200e",
    ] {
        let addr = parse_addr(raw).unwrap_or_else(|| panic!("{raw} did not parse as an address"));
        assert!(
            !is_blocked(addr),
            "{raw} is closed, but it is an ordinary public address"
        );
    }
}

#[test]
fn check_host_rejects_what_is_visible_by_name() {
    for host in [
        "",
        "localhost",
        "app.localhost",
        "LOCALHOST",
        "wiki",
        "127.0.0.1",
        "169.254.169.254",
        "[::1]",
    ] {
        assert!(
            matches!(check_host(host), Err(Error::Blocked(_))),
            "host {host:?} passed"
        );
    }
    for host in ["example.com", "www.example.org", "example.com.", "8.8.8.8"] {
        assert_eq!(check_host(host), Ok(()), "host {host:?} rejected");
    }
}

#[test]
fn safe_url_rejects_schemes_and_internal_targets() {
    for raw in [
        "file:///etc/passwd",
        "gopher://example.com",
        "ftp://example.com/pub",
        "not a url",
        "http://127.0.0.1:8090/dashboard",
        "https://192.168.1.1/",
        "http://169.254.169.254/latest/meta-data/",
        "http://[::1]:8088/admin/",
        "http://localhost:5432/",
    ] {
        assert!(
            matches!(safe_url(raw), Err(Error::Blocked(_))),
            "{raw:?} passed"
        );
    }
    assert!(
        safe_url("https://example.com/contacts").is_ok(),
        "an ordinary address was rejected"
    );
}

#[test]
fn dial_guard_checks_the_address_it_is_given() {
    for address in [
        "127.0.0.1:80",
        "169.254.169.254:80",
        "[::1]:8088",
        "10.0.0.5:5432",
        "garbage",
    ] {
        assert!(
            matches!(guard_dial("tcp4", address), Err(Error::Blocked(_))),
            "dialing {address} allowed"
        );
    }
    assert_eq!(
        guard_dial("tcp4", "8.8.8.8:443"),
        Ok(()),
        "dialing a public address refused"
    );
    // A non-TCP network is either UDP or a unix socket: the fetcher needs
    // neither, and a unix socket leads straight to a database.
    assert!(matches!(
        guard_dial("unix", "/var/run/postgresql/.s.PGSQL.5432"),
        Err(Error::Blocked(_))
    ));
}

/// The point of the module: a live server on loopback stays unreachable even
/// when its name passed. The request goes around `fetch` on purpose, skipping
/// address parsing, so only the barrier in the resolver answers here.
#[tokio::test]
async fn loopback_server_stays_unreachable() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = conn.read(&mut buf).await;
            let _ = conn
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecret")
                .await;
        }
    });

    let client = netguard::new(&netguard::Options::default());
    // localhost goes through the resolver; the digits go through the policy
    let by_name = client
        .http
        .get(format!("http://localhost:{port}/"))
        .send()
        .await;
    let err = by_name.expect_err("a connection to loopback opened by name");
    assert!(
        blocked_source(&err),
        "the refusal does not come from the barrier: {err}"
    );

    let fetched = client.fetch(&format!("http://127.0.0.1:{port}/")).await;
    assert!(
        matches!(fetched, Err(Error::Blocked(_))),
        "fetch reached loopback: {fetched:?}"
    );
}

fn blocked_source(err: &reqwest::Error) -> bool {
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(e) = cur {
        if matches!(e.downcast_ref::<Error>(), Some(Error::Blocked(_))) {
            return true;
        }
        cur = e.source();
    }
    false
}

#[test]
fn redirect_policy() {
    let file = Url::parse("file:///etc/passwd").unwrap();
    assert!(
        matches!(check_redirect(&file, 0, 2), Err(Error::Blocked(_))),
        "redirect to file:// allowed"
    );

    let next = Url::parse("https://example.com/next").unwrap();
    assert_eq!(
        check_redirect(&next, 2, 2),
        Ok(()),
        "the second redirect was refused"
    );
    assert!(
        matches!(check_redirect(&next, 3, 2), Err(Error::Blocked(_))),
        "redirects never end"
    );

    // An address in digits is not resolved, so the policy checks it
    let inward = Url::parse("http://169.254.169.254/latest/").unwrap();
    assert!(
        matches!(check_redirect(&inward, 1, 2), Err(Error::Blocked(_))),
        "a redirect inward allowed"
    );
}

#[tokio::test]
async fn read_capped_marks_truncation() {
    let stream = |s: &'static [u8]| futures::stream::iter(s.chunks(3).map(Ok::<_, std::io::Error>));

    let (body, truncated) = read_capped(stream(b"12345"), 10).await.unwrap();
    assert!(!truncated && body == b"12345", "short response read wrong");

    let (body, truncated) = read_capped(stream(b"1234567890"), 10).await.unwrap();
    assert!(
        !truncated && body.len() == 10,
        "a response at the cap counted as truncated"
    );

    let (body, truncated) = read_capped(stream(&[b'a'; 4096]), 10).await.unwrap();
    assert!(truncated && body.len() == 10, "the cap did not work");
}
