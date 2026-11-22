// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! False detections taken from live sites. Each test keeps one case: what
//! exactly a feature fragment matched in someone else's text, and what the
//! answer should be instead. Markup is trimmed to the relevant part; full
//! responses are not stored in the repository.

use http::HeaderMap;
use regex::Regex;

use super::{headers, score};
use crate::select_winners;
use crate::signatures::CMS_SIGNATURES;

/// www.tbm.ru came out as "Evolution CMS and Nuxt.js": the feature `evo-`
/// matched inside the transliteration "derevo-alyuminievaya", and there is no
/// trace of Evolution on the site. Another service on the same domain came out
/// as NGCMS, which is not there either.
#[test]
fn transliteration_does_not_name_evolution() {
    let html = concat!(
        r#"<html><head><script>window.__NUXT__=(function(a){return {data:[{}]}})()</script></head>"#,
        r#"<body><a href="/derevo-alyuminievaya-sistema/derevo-alyuminievaya-sistema-gemini/">Gemini</a>"#,
        r#"<a href="https://ml.rbc.ru/arctic26s-c2-kmiva-dizanalevo-m">РБК</a>"#,
        r#"<div data-v-7f3a1c2b>Двери</div></body></html>"#,
    );
    let got = select_winners(&score(html, &headers(&[("x-powered-by", "Nuxt")])));
    assert_eq!(got, vec!["Nuxt.js".to_owned()]);
}

/// www.kommersant.ru was named Jekyll because of `_site/` matching inside
/// someone else's image path. `_site/` is Jekyll's build directory and never
/// appears in served URLs, so the feature was removed rather than anchored.
#[test]
fn build_directory_path_does_not_name_jekyll() {
    let html = r#"<html><body><img src="/Issues.photo2/WEEKEND_Online_Site/2026/08/28/KMO_17838.jpg"></body></html>"#;
    let got = select_winners(&score(html, &HeaderMap::new()));
    assert!(got.is_empty(), "this page runs no engine, got {got:?}");
}

/// The `csrf-param` and `csrf-token` pair is written by Rails, Yii, InstantCMS
/// and hand-made backends: www.rbc.ru was named Rails one day and Laravel the
/// next. Only the value `authenticity_token` is specific to Rails.
#[test]
fn shared_csrf_token_names_no_engine() {
    let shared = r#"<html><head><meta name="csrf-token" content="9d1f"><meta name="csrf-param" content="csrf_token"></head></html>"#;
    assert_eq!(
        select_winners(&score(shared, &HeaderMap::new())),
        Vec::<String>::new()
    );

    let rails = r#"<html><head><meta name="csrf-param" content="authenticity_token"><meta name="csrf-token" content="9d1f"></head></html>"#;
    assert_eq!(
        select_winners(&score(rails, &HeaderMap::new())),
        vec!["Ruby on Rails".to_owned()]
    );
}

/// A rule instead of a list: a short fragment feature must start at a word
/// boundary. This class of bug is invisible by eye: a string looks specific
/// right until the day someone's slug ends with it, and the site gets an engine
/// it does not run. Features with a delimiter (`/`, `<`, `"`, `=`, `.`) are
/// exempt: the delimiter is the boundary.
#[test]
fn bare_fragments_are_word_anchored() {
    let bare_fragment = Regex::new(r"^[A-Za-z0-9_-]+$").unwrap();
    for entry in CMS_SIGNATURES {
        for item in entry.sig.body {
            if item.pattern.starts_with(r"\b") {
                continue;
            }
            assert!(
                !bare_fragment.is_match(item.pattern),
                "{}: feature {:?} has no leading \\b and will match inside other words",
                entry.name,
                item.pattern
            );
        }
    }
}
