// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! Flags as Go's `flag` package reads them with `ExitOnError`: one dash or
//! two, the value after `=` or as the next word, `--` and the first word
//! without a dash end the flags. The help text, the refusals and the exit codes
//! (2 on an error, 0 on `-h`) are Go's too.

use std::time::Duration;

const USAGE: &str = "Usage: cmslens [flags] domain...\n       cmslens -file page.html\n\n";

/// The flag list as Go's `PrintDefaults` writes it, sorted by name.
const DEFAULTS: &str = concat!(
    "  -country string\n    \tScraperAPI country code, such as us or de\n",
    "  -file string\n    \tdetect from a saved HTML page instead of fetching (no headers are used)\n",
    "  -json\n    \tprint one JSON object per site\n",
    "  -lang string\n    \tAccept-Language to send (default \"en-US,en;q=0.9\")\n",
    "  -scraperapi-key string\n    \tretry blocked sites through ScraperAPI (default $SCRAPERAPI_KEY)\n",
    "  -timeout duration\n    \ttime limit per site (default 1m0s)\n",
    "  -version\n    \tprint the version and exit\n",
);

pub(crate) fn usage() -> String {
    format!("{USAGE}{DEFAULTS}")
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Set {
    pub(crate) json: bool,
    pub(crate) file: String,
    /// None keeps the default; a negative Go duration is zero.
    pub(crate) timeout: Option<Duration>,
    pub(crate) scraper_key: String,
    pub(crate) country: String,
    pub(crate) lang: String,
    pub(crate) version: bool,
    pub(crate) domains: Vec<String>,
}

/// Parses the arguments after the program name. On `-h` or an error the text
/// is already on stderr, and the result is the exit code.
pub(crate) fn parse(args: &[String]) -> Result<Set, u8> {
    let mut err = std::io::stderr();
    parse_to(args, &mut err)
}

pub(crate) fn parse_to(args: &[String], out: &mut impl std::io::Write) -> Result<Set, u8> {
    let mut set = Set::default();
    let mut rest = args;
    let mut fail = |msg: String| {
        let _ = write!(out, "{msg}\n{}", usage());
        2
    };
    while let Some(s) = rest.first() {
        if s.len() < 2 || !s.starts_with('-') {
            break;
        }
        let minuses = if s.as_bytes()[1] == b'-' {
            if s.len() == 2 {
                rest = &rest[1..];
                break;
            }
            2
        } else {
            1
        };
        let name = &s[minuses..];
        if name.is_empty() || name.starts_with('-') || name.starts_with('=') {
            return Err(fail(format!("bad flag syntax: {s}")));
        }
        rest = &rest[1..];
        let (name, value) = match name[1..].find('=') {
            Some(i) => (&name[..=i], Some(name[i + 2..].to_owned())),
            None => (name, None),
        };
        if matches!(name, "json" | "version") {
            let on = match value.as_deref() {
                None => true,
                Some(v) => match parse_bool(v) {
                    Some(b) => b,
                    None => {
                        return Err(fail(format!(
                            "invalid boolean value {} for -{name}: parse error",
                            quote(v)
                        )));
                    }
                },
            };
            if name == "json" {
                set.json = on;
            } else {
                set.version = on;
            }
            continue;
        }
        if !matches!(
            name,
            "file" | "timeout" | "scraperapi-key" | "country" | "lang"
        ) {
            if name == "help" || name == "h" {
                let _ = write!(out, "{}", usage());
                return Err(0);
            }
            return Err(fail(format!("flag provided but not defined: -{name}")));
        }
        let value = match value {
            Some(v) => v,
            None => match rest.first() {
                Some(v) => {
                    rest = &rest[1..];
                    v.clone()
                }
                None => return Err(fail(format!("flag needs an argument: -{name}"))),
            },
        };
        match name {
            "file" => set.file = value,
            "scraperapi-key" => set.scraper_key = value,
            "country" => set.country = value,
            "lang" => set.lang = value,
            _ => match parse_duration(&value) {
                Some(d) => set.timeout = Some(d),
                None => {
                    return Err(fail(format!(
                        "invalid value {} for flag -{name}: parse error",
                        quote(&value)
                    )));
                }
            },
        }
    }
    set.domains = rest.to_vec();
    Ok(set)
}

/// `strconv.ParseBool`.
fn parse_bool(s: &str) -> Option<bool> {
    match s {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

fn quote(s: &str) -> String {
    format!("{s:?}")
}

/// `time.ParseDuration`: a signed sequence of decimal numbers, each with an
/// optional fraction and a unit (ns, us, µs, μs, ms, s, m, h); "0" alone needs
/// no unit. A negative duration is returned as zero: it is already over.
pub(crate) fn parse_duration(s: &str) -> Option<Duration> {
    let (neg, mut rest) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    if rest == "0" {
        return Some(Duration::ZERO);
    }
    if rest.is_empty() {
        return None;
    }
    let mut total: u128 = 0;
    while !rest.is_empty() {
        let int_len = rest.bytes().take_while(u8::is_ascii_digit).count();
        let (int, after) = rest.split_at(int_len);
        let (frac, after) = match after.strip_prefix('.') {
            Some(a) => {
                let n = a.bytes().take_while(u8::is_ascii_digit).count();
                a.split_at(n)
            }
            None => ("", after),
        };
        if int.is_empty() && frac.is_empty() {
            return None;
        }
        let unit_len = after
            .find(|c: char| c == '.' || c.is_ascii_digit())
            .unwrap_or(after.len());
        let (unit, after) = after.split_at(unit_len);
        let scale: u128 = match unit {
            "ns" => 1,
            "us" | "µs" | "μs" => 1_000,
            "ms" => 1_000_000,
            "s" => 1_000_000_000,
            "m" => 60_000_000_000,
            "h" => 3_600_000_000_000,
            _ => return None,
        };
        let whole: u128 = if int.is_empty() { 0 } else { int.parse().ok()? };
        let mut part = whole.checked_mul(scale)?;
        if !frac.is_empty() {
            let digits = frac.len().min(30);
            let f: u128 = frac[..digits].parse().ok()?;
            part += f * scale / 10u128.pow(u32::try_from(digits).ok()?);
        }
        total = total.checked_add(part)?;
        // Go keeps a duration in an int64 of nanoseconds
        if total > u128::from(u64::MAX >> 1) + u128::from(neg) {
            return None;
        }
        rest = after;
    }
    if neg {
        return Some(Duration::ZERO);
    }
    Some(Duration::from_nanos(u64::try_from(total).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> (Result<Set, u8>, String) {
        let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let mut out = Vec::new();
        let res = parse_to(&args, &mut out);
        (res, String::from_utf8(out).unwrap())
    }

    // The texts and codes are taken from go run ./cmd/cmslens
    #[test]
    fn flags_as_go() {
        let usage = usage();
        assert_eq!(run(&["-h"]), (Err(0), usage.clone()));
        assert_eq!(run(&["--help"]), (Err(0), usage.clone()));
        assert_eq!(
            run(&["-x"]),
            (
                Err(2),
                format!("flag provided but not defined: -x\n{usage}")
            )
        );
        assert_eq!(
            run(&["-json=maybe"]),
            (
                Err(2),
                format!("invalid boolean value \"maybe\" for -json: parse error\n{usage}")
            )
        );
        assert_eq!(
            run(&["-timeout", "5", "x"]),
            (
                Err(2),
                format!("invalid value \"5\" for flag -timeout: parse error\n{usage}")
            )
        );
        assert_eq!(
            run(&["-file"]),
            (Err(2), format!("flag needs an argument: -file\n{usage}"))
        );
        assert_eq!(
            run(&["---json"]),
            (Err(2), format!("bad flag syntax: ---json\n{usage}"))
        );

        let (set, _) = run(&[
            "-json",
            "--timeout=90s",
            "-lang",
            "ru",
            "a.com",
            "-json",
            "b.com",
        ]);
        assert_eq!(
            set,
            Ok(Set {
                json: true,
                timeout: Some(Duration::from_secs(90)),
                lang: "ru".to_owned(),
                domains: vec!["a.com".to_owned(), "-json".to_owned(), "b.com".to_owned()],
                ..Set::default()
            })
        );
        let (set, _) = run(&["-json=false", "--", "-x"]);
        assert_eq!(
            set,
            Ok(Set {
                domains: vec!["-x".to_owned()],
                ..Set::default()
            })
        );
    }

    #[test]
    fn durations_as_go() {
        let cases = [
            ("0", Some(Duration::ZERO)),
            ("60s", Some(Duration::from_secs(60))),
            ("1m30s", Some(Duration::from_secs(90))),
            ("1.5h", Some(Duration::from_secs(5400))),
            ("300ms", Some(Duration::from_millis(300))),
            (".5s", Some(Duration::from_millis(500))),
            ("2µs", Some(Duration::from_micros(2))),
            ("-1s", Some(Duration::ZERO)),
            ("5", None),
            ("", None),
            ("s", None),
            ("1x", None),
            ("1.s", Some(Duration::from_secs(1))),
            ("9999999999h", None),
        ];
        for (input, want) in cases {
            assert_eq!(parse_duration(input), want, "{input}");
        }
    }
}
