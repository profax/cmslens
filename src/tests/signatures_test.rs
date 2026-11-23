// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use std::collections::HashSet;

use super::{headers, score};
use crate::signatures::{CMS_SIGNATURES, COMPILED, FRONTEND_LAYER, is_frontend};
use crate::{CmsScore, select_winners};

/// Builds a score list from name and score pairs.
fn scores(pairs: &[(&str, f64)]) -> Vec<CmsScore> {
    pairs
        .iter()
        .map(|(name, score)| CmsScore {
            name: (*name).to_owned(),
            score: *score,
        })
        .collect()
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn select_winners_competing_backends() {
    assert_eq!(
        select_winners(&scores(&[("Joomla", 3.0), ("Winter CMS", 1.0)])),
        names(&["Joomla"])
    );
    assert_eq!(
        select_winners(&scores(&[
            ("Drupal", 2.0),
            ("WordPress", 5.0),
            ("Joomla", 1.0)
        ])),
        names(&["WordPress"])
    );
    assert_eq!(
        select_winners(&scores(&[("MODX", 1.0), ("Evolution CMS", 3.0)])),
        names(&["Evolution CMS"])
    );
    assert_eq!(
        select_winners(&scores(&[("October CMS", 2.0), ("Winter CMS", 3.0)])),
        names(&["Winter CMS"])
    );
    assert_eq!(
        select_winners(&scores(&[("1C-Bitrix", 4.0), ("WordPress", 2.0)])),
        names(&["1C-Bitrix"])
    );
    assert_eq!(
        select_winners(&scores(&[("Webasyst", 2.0), ("Shop-Script", 4.0)])),
        names(&["Shop-Script"])
    );
}

#[test]
fn select_winners_exactly_one_backend() {
    let result = select_winners(&scores(&[
        ("WordPress", 2.0),
        ("Joomla", 1.0),
        ("Drupal", 3.0),
        ("OpenCart", 1.0),
        ("Bitrix", 1.0),
    ]));
    let backends = result.iter().filter(|n| !is_frontend(n)).count();
    assert_eq!(backends, 1, "want exactly one backend engine in {result:?}");
}

#[test]
fn select_winners_headless_pairs() {
    let cases = [
        (
            scores(&[("WordPress", 3.0), ("Next.js", 2.0)]),
            "Next.js",
            "WordPress",
        ),
        (
            scores(&[("Astro", 3.0), ("Contentful", 2.0)]),
            "Astro",
            "Contentful",
        ),
        (
            scores(&[("1C-Bitrix", 4.0), ("Nuxt.js", 2.0)]),
            "Nuxt.js",
            "1C-Bitrix",
        ),
    ];
    for (s, front, back) in cases {
        let result = select_winners(&s);
        assert_eq!(result.len(), 2, "{s:?}: want 2 results, got {result:?}");
        assert!(
            result.iter().any(|n| n == front) && result.iter().any(|n| n == back),
            "{s:?}: got {result:?}"
        );
    }
}

#[test]
fn select_winners_competing_frontends() {
    assert_eq!(
        select_winners(&scores(&[("Next.js", 3.0), ("React", 2.0)])),
        names(&["Next.js"])
    );
    assert_eq!(
        select_winners(&scores(&[("Gatsby", 2.0), ("React", 1.0)])),
        names(&["Gatsby"])
    );
}

/// The October and Winter theme path weighs 0.5 and appears on unrelated
/// sites: on its own it gives no answer, and next to a real engine it does not
/// stop that engine from winning.
#[test]
fn select_winners_ignores_below_threshold() {
    let half = scores(&[("October CMS", 0.5), ("Winter CMS", 0.5)]);
    assert_eq!(select_winners(&half), Vec::<String>::new());
    let mut with_engine = half;
    with_engine.push(CmsScore {
        name: "ePages".to_owned(),
        score: 1.0,
    });
    assert_eq!(select_winners(&with_engine), names(&["ePages"]));
}

#[test]
fn select_winners_edge_cases() {
    assert!(
        select_winners(&[]).is_empty(),
        "empty input must give empty output"
    );
    assert_eq!(
        select_winners(&scores(&[("Joomla", 3.0)])),
        names(&["Joomla"])
    );
    assert_eq!(
        select_winners(&scores(&[("Astro", 2.0)])),
        names(&["Astro"])
    );

    let result = select_winners(&scores(&[("WordPress", 3.0), ("Next.js", 5.0)]));
    assert_eq!(
        result,
        names(&["Next.js", "WordPress"]),
        "want order by descending score"
    );

    let max2 = select_winners(&scores(&[
        ("WordPress", 5.0),
        ("Joomla", 3.0),
        ("Drupal", 2.0),
        ("Next.js", 4.0),
        ("Astro", 3.0),
        ("Vue", 2.0),
    ]));
    assert!(max2.len() <= 2, "want at most 2 results, got {max2:?}");
}

#[test]
fn frontend_layer_membership() {
    for fw in [
        "Next.js", "Nuxt.js", "Astro", "Svelte", "Gatsby", "React", "Vue", "Angular", "Vite",
    ] {
        assert!(
            FRONTEND_LAYER.contains(&fw),
            "frontend layer must include {fw}"
        );
    }
    for cms in [
        "WordPress",
        "Joomla",
        "1C-Bitrix",
        "Drupal",
        "Winter CMS",
        "October CMS",
        "MODX",
    ] {
        assert!(
            !is_frontend(cms),
            "frontend layer must not include {cms} (it is a backend)"
        );
    }
}

/// The table has 556 signatures. The test keeps the number visible on the next
/// edit of the table, and compiling the table here is what proves every
/// expression is valid.
#[test]
fn all_signatures_compile() {
    assert_eq!(CMS_SIGNATURES.len(), 556);
    assert_eq!(COMPILED.len(), 556);
    // Expressions are built on first use; build each one here
    for entry in COMPILED.iter() {
        let headers = entry.headers.iter().flat_map(|(_, items)| items);
        for f in entry.body.iter().chain(headers) {
            f.re();
        }
    }
    let mut seen = HashSet::new();
    for entry in CMS_SIGNATURES {
        assert!(
            seen.insert(entry.name),
            "duplicate CMS name: {}",
            entry.name
        );
        assert!(
            !entry.sig.body.is_empty() || !entry.sig.headers.is_empty(),
            "{}: signature without a single feature",
            entry.name
        );
        let mut header_names = HashSet::new();
        for (name, _) in entry.sig.headers {
            assert!(
                header_names.insert(*name),
                "{}: header {name} twice",
                entry.name
            );
            assert_eq!(
                *name,
                name.to_ascii_lowercase(),
                "{}: header names are lower case",
                entry.name
            );
        }
    }
}

/// A real case: kitanasushi.ru answers with a Next.js header but is built on
/// the StarterApp SaaS platform, whose CDN domains sit on every client page.
/// While the platform was missing from the table, the answer was the framework
/// alone, i.e. the presentation layer instead of the engine. Another service
/// answered "VamShop" for the same site, though there is no trace of that CMS
/// at all.
#[test]
fn starter_app_wins_over_frontend_layer() {
    let html = concat!(
        r#"<html><head><link rel="preconnect" href="https://content.cdn.starterapp.ru">"#,
        r#"<script src="/_next/static/chunks/main.js"></script></head><body>"#,
        r#"<img src="https://cdn.starterapp.ru/logo.png">"#,
        r#"<script>fetch("https://api.starterapp.ru/v1/menu")</script></body></html>"#,
    );
    let got = select_winners(&score(html, &headers(&[("x-powered-by", "Next.js")])));
    assert_eq!(
        got,
        names(&["StarterApp", "Next.js"]),
        "want the pair of platform and presentation layer"
    );
    assert!(
        !got.iter().any(|n| n == "VamShop"),
        "VamShop has nothing to do with this site"
    );
}

/// While the Go module lives next to the crate, its table and this one are the
/// same table: every entry, feature, weight and header in the same order. A
/// feature added to one side only fails here. The test goes away with the Go
/// module.
#[test]
fn table_matches_the_go_source() {
    let go =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/signatures.go")).unwrap();
    let start = go.find("var cmsSignatures").expect("the Go table");
    let end = start + go[start..].find("\n}\n").expect("the end of the Go table");
    let token = regex::Regex::new(
        r#"\{"([^"]+)", signature\{|"([a-z0-9-]+)":\s*\{|\b(bw|hw|b|h)\(`([^`]*)`(?:, ([0-9.]+))?\)"#,
    )
    .unwrap();
    let mut from_go = Vec::new();
    for c in token.captures_iter(&go[start..end]) {
        if let Some(name) = c.get(1) {
            from_go.push(format!("entry {}", name.as_str()));
        } else if let Some(header) = c.get(2) {
            from_go.push(format!("header {}", header.as_str()));
        } else {
            let kind = if c[3].starts_with('b') {
                "body"
            } else {
                "header item"
            };
            let weight: f64 = match c.get(5) {
                Some(w) => w.as_str().parse().unwrap(),
                None if kind == "body" => 1.0,
                None => 2.0,
            };
            from_go.push(format!("{kind} {} {weight}", &c[4]));
        }
    }
    let mut from_rust = Vec::new();
    for entry in CMS_SIGNATURES {
        from_rust.push(format!("entry {}", entry.name));
        for item in entry.sig.body {
            from_rust.push(format!("body {} {}", item.pattern, item.weight));
        }
        for (header, items) in entry.sig.headers {
            from_rust.push(format!("header {header}"));
            for item in *items {
                from_rust.push(format!("header item {} {}", item.pattern, item.weight));
            }
        }
    }
    assert!(
        from_go.len() > CMS_SIGNATURES.len() * 2,
        "the Go table was not read: {} tokens",
        from_go.len()
    );
    for (i, (g, r)) in from_go.iter().zip(&from_rust).enumerate() {
        assert_eq!(g, r, "token {i} differs");
    }
    assert_eq!(from_go.len(), from_rust.len());
}
