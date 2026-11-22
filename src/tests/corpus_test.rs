// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::path::Path;

use http::{HeaderMap, HeaderName, HeaderValue};

use crate::gostr::go_string;
use crate::scan::score_response;
use crate::select_winners;

/// Runs the table against responses of live sites: this is how a new batch of
/// features is checked for false positives. Markup of other people's sites is
/// not stored in the repository; testdata/corpus.sh downloads it into the
/// directory named by `CMSLENS_CORPUS`. `testdata/corpus.tsv` gives the expected
/// answer per domain; domains without one are only printed.
///
/// `cargo test corpus -- --ignored --nocapture` with the variable set. Run
/// without it, the test fails: a check with nothing to check is not a pass.
#[test]
#[ignore = "needs CMSLENS_CORPUS, see testdata/corpus.sh"]
fn corpus() {
    let dir = std::env::var("CMSLENS_CORPUS").expect("CMSLENS_CORPUS is not set");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let expect = read_expect(&root.join("testdata/corpus.tsv"));
    let mut checked = 0;
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let page = entry.unwrap().path();
        if page.extension().is_none_or(|e| e != "html") {
            continue;
        }
        let domain = page.file_stem().unwrap().to_string_lossy().into_owned();
        let body = std::fs::read(&page).unwrap();
        if body.is_empty() {
            continue;
        }
        let raw = std::fs::read(Path::new(&dir).join(format!("{domain}.headers"))).unwrap();
        let scores = score_response(&body, &go_string(&body), &last_header_block(&raw));
        let mut got = select_winners(&scores);
        if got.is_empty() {
            got = vec!["-".to_owned()];
        }
        checked += 1;
        match expect.get(&domain) {
            None => println!("{domain:<32} {:<28} {scores:?}", got.join(",")),
            Some(want) if !same_names(&got, want) => {
                failures.push(format!(
                    "{domain}: got {}, want {}; scores {scores:?}",
                    got.join(","),
                    want.join(",")
                ));
            }
            Some(_) => {}
        }
    }
    assert!(checked > 0, "no pages in {dir}: the check checked nothing");
    assert!(
        failures.is_empty(),
        "{} of {checked} pages:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

pub(crate) fn read_expect(path: &Path) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let (domain, names) = line.split_once('\t').unwrap_or((line, ""));
        if !names.is_empty() && !domain.starts_with('#') {
            out.insert(
                domain.to_owned(),
                names.split(',').map(str::to_owned).collect(),
            );
        }
    }
    out
}

fn same_names(a: &[String], b: &[String]) -> bool {
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    a.sort();
    b.sort();
    a == b
}

/// The headers of the final response: curl -L writes the headers of every
/// redirect hop into the file one after another. A line that is not a header
/// is skipped, as Go's textproto reader skips nothing but stops there; the
/// files curl writes have no such lines.
pub(crate) fn last_header_block(raw: &[u8]) -> HeaderMap {
    let text = go_string(raw);
    let block = text.trim().rsplit("\r\n\r\n").next().unwrap_or("");
    let mut h = HeaderMap::new();
    for line in block.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            break;
        };
        let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.trim().as_bytes()),
            HeaderValue::from_str(value.trim()),
        ) else {
            continue;
        };
        h.append(name, value);
    }
    h
}
