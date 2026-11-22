// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use crate::normalize_domain;

#[test]
fn normalize_domain_keeps_the_host() {
    let cases = [
        ("example.com", "example.com"),
        ("  Example.COM  ", "example.com"),
        ("www.example.com", "example.com"),
        ("https://www.example.com/a?b#c", "example.com"),
        ("http://example.com:8080/path", "example.com"),
        ("example.com?x=1", "example.com"),
        ("example.com#top", "example.com"),
        ("http://user@example.com", "example.com"),
        ("shop.example.co.uk", "shop.example.co.uk"),
        ("xn--d1abbgf6aiiy.xn--p1ai", "xn--d1abbgf6aiiy.xn--p1ai"),
        (
            "HTTPS://XN--D1ABBGF6AIIY.XN--P1AI",
            "xn--d1abbgf6aiiy.xn--p1ai",
        ),
        // A name in another script is typed as is and fetched as punycode
        ("президент.рф", "xn--d1abbgf6aiiy.xn--p1ai"),
        ("https://www.Пример.РФ/путь", "xn--e1afmkfd.xn--p1ai"),
        ("пример.com", "xn--e1afmkfd.com"),
        ("shop.испытание.рф", "shop.xn--80akhbyknj4f.xn--p1ai"),
    ];
    for (input, want) in cases {
        assert_eq!(
            normalize_domain(input).as_deref(),
            Some(want),
            "normalize_domain({input:?})"
        );
    }
}

/// An address instead of a name is the shortest way into an internal network,
/// and a name without a public suffix is not a site on the internet.
#[test]
fn normalize_domain_rejects_what_is_not_a_domain() {
    let long = format!("{}.рф", "я".repeat(60));
    for input in [
        "",
        "   ",
        "www.",
        "localhost",
        "intranet",
        "a.b",
        "1.2.3.4",
        "https://192.168.1.1/admin",
        "[::1]",
        "http://[::1]:8080/",
        "-bad.example.com",
        "ex ample.com",
        "example.c0m",
        // Longer than a DNS label once in punycode
        long.as_str(),
    ] {
        assert_eq!(normalize_domain(input), None, "normalize_domain({input:?})");
    }
}

/// The corners of Go's url.Parse that decide between an answer and a
/// rejection. Each answer is what the Go package gave for the same input
/// (go run with cmslens v0.1.4, 22.11.2026).
#[test]
fn normalize_domain_reads_urls_as_go() {
    let cases: &[(&str, Option<&str>)] = &[
        ("http://user:pass@example.com:80/", Some("example.com")),
        ("user name@example.com", None),
        ("u%zz@example.com", None),
        ("u%41@example.com", Some("example.com")),
        ("a@b@example.com", Some("example.com")),
        ("example.com:", Some("example.com")),
        ("example.com:8a", None),
        ("example.com:80:90", None),
        (
            "%d0%bf%d1%80%d0%b8%d0%bc%d0%b5%d1%80.com",
            Some("xn--e1afmkfd.com"),
        ),
        ("%61bc.com", None),
        ("ex%2eample.com", None),
        ("exa%25mple.com", None),
        ("exa_mple.com", Some("exa_mple.com")),
        ("ex~ample.com", None),
        ("ex\u{1}ample.com", None),
        ("[example.com]", None),
        ("[::ffff:1.2.3.4]", None),
        ("İstanbul.com", Some("istanbul.com")),
        ("ΣΑΣ.gr", Some("xn--mxa9ab.gr")),
        ("www.www.example.com", Some("www.example.com")),
        ("HTTP://WWW.EXAMPLE.COM", Some("example.com")),
        ("https://http://example.com", Some("example.com")),
        ("example.com.", None),
        ("a..b.com", Some("a..b.com")),
        ("1.2.3.com", Some("1.2.3.com")),
        ("xn--.com", None),
        ("example.xn--", None),
        ("\u{a0}example.com\u{a0}", Some("example.com")),
    ];
    for (input, want) in cases {
        assert_eq!(
            normalize_domain(input).as_deref(),
            *want,
            "normalize_domain({input:?})"
        );
    }
}
