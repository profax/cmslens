// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use crate::Options;
use crate::scan::scraper_api_url;

/// The ScraperAPI key travels in the query of the request, and the request
/// went over plain http: anyone on the path between the scanner and
/// ScraperAPI could read the key and spend its credits.
#[test]
fn scraper_api_url_uses_https() {
    let opts = Options {
        scraper_api_key: "k&y".to_owned(),
        ..Options::default()
    };
    assert_eq!(
        scraper_api_url(&opts, "https://example.com/?a=1"),
        "https://api.scraperapi.com?api_key=k%26y&url=https%3A%2F%2Fexample.com%2F%3Fa%3D1"
    );
    let opts = Options {
        scraper_api_key: "key".to_owned(),
        scraper_country: "de".to_owned(),
        ..Options::default()
    };
    assert_eq!(
        scraper_api_url(&opts, "https://example.com/?a=1"),
        "https://api.scraperapi.com?api_key=key&url=https%3A%2F%2Fexample.com%2F%3Fa%3D1&country_code=de"
    );
}
