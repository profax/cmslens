// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! Tests file for file with the Go tests they were translated from
//! (`match_test.go` is `match_test.rs`), with the same names in snake case.

mod corpus_test;
mod domain_test;
mod falsepositive_test;
mod license_test;
mod match_test;
mod netguard_test;
mod punycode_test;
mod scan_test;
mod signatures_test;

use http::{HeaderMap, HeaderName, HeaderValue};

use crate::CmsScore;
use crate::gostr::go_string;
use crate::scan::score_response;

/// `scoreResponse` over a string page, as the Go tests call it.
pub(crate) fn score(html: &str, headers: &HeaderMap) -> Vec<CmsScore> {
    score_response(html.as_bytes(), &go_string(html.as_bytes()), headers)
}

pub(crate) fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in pairs {
        h.append(
            HeaderName::from_bytes(k.as_bytes()).unwrap(),
            HeaderValue::from_str(v).unwrap(),
        );
    }
    h
}
