// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! How Go treats a string, where the answer depends on it: a body that is not
//! UTF-8, lower case, and `%q` in error texts.

use std::borrow::Cow;
use std::fmt::Write as _;

/// Reads bytes the way Go ranges over a string: every byte that does not
/// start a valid sequence is one U+FFFD. `String::from_utf8_lossy` replaces a
/// broken sequence as a whole instead, so `[^>]{0,9}` would count differently
/// on a page in windows-1251, and a title in JSON would come out shorter.
pub(crate) fn go_string(bytes: &[u8]) -> Cow<'_, str> {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(bytes.len() + bytes.len() / 2);
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for _ in chunk.invalid() {
            out.push(char::REPLACEMENT_CHARACTER);
        }
    }
    Cow::Owned(out)
}

/// `strings.ToLower`: one rune for one rune by the simple mapping. The full
/// mapping of `str::to_lowercase` turns `İ` into two characters and a final
/// `Σ` into `ς`, and a host would get another punycode than in Go.
pub(crate) fn to_lower(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{130}' => 'i',
            c => c.to_lowercase().next().unwrap_or(c),
        })
        .collect()
}

/// `%q` of Go for an error text: quotes and backslashes escaped, control
/// characters written as escapes, everything else as is.
pub(crate) fn go_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{b}' => out.push_str("\\v"),
            c if u32::from(c) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
