// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! cmslens tells which CMS, site builder or web framework a website runs, from
//! its HTML and response headers.
//!
//! [`detect`] works offline on a page you already have. [`scan`] keeps only the
//! host of the name or URL it is given and fetches the home page of that host
//! itself, refusing to connect to private and internal addresses. When a site
//! blocks it, [`scan`] can retry through ScraperAPI.
//!
//! A result names at most one engine and at most one presentation layer on
//! top of it: a headless site answers two names, such as
//! `["Next.js", "WordPress"]`, an ordinary one `["WordPress"]`. The name with
//! the higher score comes first; on a tie the engine does.
//!
//! The crate gives the same answers as the Go package it was translated from,
//! byte for byte where the Go package wrote text: the expressions of the table
//! keep RE2's ASCII meaning of `\b`, `\d` and `\s`, and a page in a legacy
//! encoding is read the way Go reads a string that is not UTF-8.

mod domain;
mod gostr;
mod matching;
mod netguard;
mod punycode;
mod re2;
mod scan;
mod signatures;

pub use domain::normalize_domain;
pub use scan::{Details, Error, Options, ScanResult, Status, detect, scan};
pub use signatures::{CmsScore, select_winners};

#[cfg(test)]
mod tests;
