// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! Command cmslens tells which CMS or framework a website runs.
//!
//! ```text
//! cmslens example.com wordpress.org
//! cmslens -json example.com
//! cmslens -file page.html
//! ```
//!
//! Flags are read the way Go's `flag` package reads them, one dash or two, so
//! scripts written for the Go build keep working.

use std::io::Write as _;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use cmslens::{Options, ScanResult, Status};
use serde::Serialize;

mod flags;

/// Sites are fetched in parallel, but not so many at once that a long list
/// looks like an attack from this machine.
const PARALLEL: usize = 8;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const EXIT_USAGE: u8 = 2;

/// The result part of a line: an error leaves it at its zero value, as the Go
/// build printed it.
#[derive(Serialize)]
struct LineResult {
    status: &'static str,
    detected: Vec<String>,
    #[serde(rename = "wafBehind")]
    waf_behind: bool,
    details: cmslens::Details,
}

impl From<ScanResult> for LineResult {
    fn from(r: ScanResult) -> Self {
        Self {
            status: r.status.as_str(),
            detected: r.detected,
            waf_behind: r.waf_behind,
            details: r.details,
        }
    }
}

#[derive(Serialize)]
struct Line {
    input: String,
    result: LineResult,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut set = match flags::parse(&args) {
        Ok(set) => set,
        Err(code) => return ExitCode::from(code),
    };
    // Read here rather than set as the flag's default: the help text prints
    // defaults, and the key would show on the screen
    if set.scraper_key.is_empty() {
        set.scraper_key = std::env::var("SCRAPERAPI_KEY").unwrap_or_default();
    }

    if set.version {
        println!("cmslens {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if !set.file.is_empty() {
        return detect_file(&set.file, set.json);
    }
    if set.domains.is_empty() {
        eprint!("{}", flags::usage());
        return ExitCode::from(EXIT_USAGE);
    }

    let opts = Options {
        scraper_api_key: set.scraper_key,
        scraper_country: set.country,
        accept_language: set.lang,
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let lines = runtime.block_on(scan_all(
        set.domains,
        opts,
        set.timeout.unwrap_or(DEFAULT_TIMEOUT),
    ));
    let mut failed = false;
    for l in lines {
        failed = failed || !l.error.is_empty();
        if let Err(e) = print_line(&l, set.json) {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Scans the sites in parallel and returns the lines in input order.
async fn scan_all(inputs: Vec<String>, opts: Options, timeout: Duration) -> Vec<Line> {
    let slots = Arc::new(tokio::sync::Semaphore::new(PARALLEL));
    let opts = Arc::new(opts);
    let tasks: Vec<_> = inputs
        .into_iter()
        .map(|input| {
            let (slots, opts) = (Arc::clone(&slots), Arc::clone(&opts));
            tokio::spawn(async move {
                let _slot = slots.acquire_owned().await;
                let res = tokio::time::timeout(timeout, cmslens::scan(&input, &opts)).await;
                match res {
                    Ok(Ok(r)) => Line {
                        input,
                        result: r.into(),
                        error: String::new(),
                    },
                    Ok(Err(e)) => Line {
                        input,
                        result: zero_result(),
                        error: e.to_string(),
                    },
                    Err(_) => Line {
                        input,
                        result: zero_result(),
                        error: "context deadline exceeded".to_owned(),
                    },
                }
            })
        })
        .collect();
    let mut lines = Vec::with_capacity(tasks.len());
    for t in tasks {
        // A task panics only on a broken table, which the tests rule out
        lines.push(t.await.expect("a scan task does not panic"));
    }
    lines
}

fn zero_result() -> LineResult {
    LineResult {
        status: "",
        detected: Vec::new(),
        waf_behind: false,
        details: cmslens::Details::default(),
    }
}

fn detect_file(path: &str, as_json: bool) -> ExitCode {
    let body = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("open {path}: {}", os_error_text(&e));
            return ExitCode::FAILURE;
        }
    };
    let detected = cmslens::detect(&body, &http::HeaderMap::new());
    let line = Line {
        input: path.to_owned(),
        result: LineResult {
            status: Status::Ok.as_str(),
            detected,
            waf_behind: false,
            details: cmslens::Details::default(),
        },
        error: String::new(),
    };
    if let Err(e) = print_line(&line, as_json) {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// The system's error text the way Go prints it: lower case, without "(os
/// error N)".
fn os_error_text(e: &std::io::Error) -> String {
    let text = e.to_string();
    let text = text.split(" (os error").next().unwrap_or(&text);
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_lowercase().chain(chars).collect())
}

fn print_line(l: &Line, as_json: bool) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    if as_json {
        let json = serde_json::to_string(l).map_err(|e| {
            std::io::Error::other(format!("encoding the result of {}: {e}", l.input))
        })?;
        return writeln!(out, "{}", escape_html(&json));
    }
    if !l.error.is_empty() {
        writeln!(out, "{}\terror: {}", l.input, l.error)
    } else if l.result.status == Status::Cloudflare.as_str() {
        writeln!(out, "{}\tblocked by an anti-bot page", l.input)
    } else if l.result.detected.is_empty() {
        writeln!(out, "{}\tunknown", l.input)
    } else {
        writeln!(out, "{}\t{}", l.input, l.result.detected.join(", "))
    }
}

/// `json.Marshal` of Go escapes these inside strings, and a script comparing
/// output of the Go build would see a difference otherwise. In this output
/// they never occur outside strings.
fn escape_html(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}
