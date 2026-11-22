// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! The table is written in the syntax of Go's regexp (RE2), and the `regex`
//! crate reads almost all of it the same way. The exception is the Perl
//! classes: in RE2 `\b`, `\d`, `\s` and `\w` are ASCII, in `regex` they are
//! Unicode. A Cyrillic letter would count as a word character, and `\bevo`
//! would stop matching after one; `\s` would take a no-break space. The
//! translation spells each class out as RE2 defines it.

/// RE2's `\s`: no vertical tab, unlike `(?-u:\s)` of `regex`.
const SPACE: &str = "\\t\\n\\f\\r ";
const DIGIT: &str = "0-9";
const WORD: &str = "0-9A-Za-z_";

/// Rewrites an RE2 expression into one `regex` reads with the same meaning.
///
/// # Panics
///
/// On syntax that has no translation here (`\Q…\E`, `\B`, a negated Perl
/// class inside brackets): the table is checked at build time by its tests,
/// so a panic means a feature was added that needs a rule first.
pub(crate) fn translate(go: &str) -> String {
    let mut out = String::with_capacity(go.len() + 16);
    let mut chars = go.chars().peekable();
    // Inside brackets, and whether the next character is the first of the
    // class: `[]a]` and `[^]a]` keep the bracket as a literal
    let mut in_class = false;
    let mut class_start = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let Some(e) = chars.next() else {
                    out.push('\\');
                    break;
                };
                class_start = false;
                out.push_str(&escape(e, in_class));
            }
            '[' if !in_class => {
                in_class = true;
                class_start = true;
                out.push('[');
                if chars.peek() == Some(&'^') {
                    chars.next();
                    out.push('^');
                }
            }
            '[' if chars.peek() == Some(&':') => {
                // A POSIX class [:alpha:] is ASCII in both engines
                out.push('[');
                for p in chars.by_ref() {
                    out.push(p);
                    if p == ']' {
                        break;
                    }
                }
                class_start = false;
            }
            ']' if in_class && !class_start => {
                in_class = false;
                out.push(']');
            }
            c => {
                class_start = false;
                out.push(c);
            }
        }
    }
    out
}

fn escape(e: char, in_class: bool) -> String {
    let set = match e {
        'd' => Some((DIGIT, false)),
        'D' => Some((DIGIT, true)),
        's' => Some((SPACE, false)),
        'S' => Some((SPACE, true)),
        'w' => Some((WORD, false)),
        'W' => Some((WORD, true)),
        _ => None,
    };
    match (set, in_class) {
        (Some((ranges, false)), true) => ranges.to_owned(),
        (Some((_, true)), true) => panic!("a negated Perl class \\{e} inside brackets"),
        (Some((ranges, negated)), false) => {
            format!("[{}{ranges}]", if negated { "^" } else { "" })
        }
        (None, false) if e == 'b' => "(?-u:\\b)".to_owned(),
        (None, _) if e == 'B' => panic!("\\B has no ASCII form in a Unicode expression"),
        (None, _) if e == 'Q' => panic!("\\Q…\\E is not read by the regex crate"),
        (None, _) => format!("\\{e}"),
    }
}
