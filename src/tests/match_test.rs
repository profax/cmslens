// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use std::collections::{HashMap, HashSet};

use regex::Regex;
use regex_syntax::ast::{self, Ast};

use crate::gostr::go_string;
use crate::matching::{Automaton, MIN_FACTOR_LEN, SIGNATURE_INDEX};
use crate::re2;
use crate::signatures::COMPILED;

/// A small deterministic generator: the tests must give the same text on
/// every run, and the crate needs no dependency for it.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % (n as u64)).unwrap()
    }
}

/// The feature table has almost no literals nested inside each other, so it
/// does not exercise suffix chains. Literals over a three-letter alphabet
/// produce them at once.
#[test]
fn automaton_finds_every_occurrence() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let word = |rng: &mut Rng, n: usize| -> String {
        (0..n)
            .map(|_| char::from(b"abcABC"[rng.below(6)]))
            .collect()
    };
    for _ in 0..200 {
        let mut ids = HashSet::new();
        let mut texts = Vec::new();
        while texts.len() < 8 {
            let n = 1 + rng.below(4);
            let s = word(&mut rng, n).to_ascii_lowercase();
            if ids.insert(s.clone()) {
                texts.push(s);
            }
        }
        let data = word(&mut rng, 300);

        let lower = data.to_ascii_lowercase();
        let mut want = HashSet::new();
        for (id, text) in texts.iter().enumerate() {
            for end in text.len() - 1..lower.len() {
                if &lower[end + 1 - text.len()..=end] == text {
                    want.insert((id, end));
                }
            }
        }
        let mut got = HashSet::new();
        Automaton::new(&texts).scan(data.as_bytes(), |id, end| {
            got.insert((id, end));
        });
        assert_eq!(got, want, "literals {texts:?} in {data:?}");
    }
}

/// A feature without a literal runs its expression over every page, and the
/// number of such features would grow back to the number of platforms.
#[test]
fn every_body_feature_narrows_by_literal() {
    for (i, entry) in COMPILED.iter().enumerate() {
        for (j, f) in entry.body.iter().enumerate() {
            let p = &SIGNATURE_INDEX.plans[i][j];
            assert!(
                p.literal.is_some() || !p.factors.is_empty(),
                "{}: feature {:?} has no literal piece of {MIN_FACTOR_LEN} bytes or more",
                entry.name,
                f.go
            );
        }
    }
}

/// The index must answer exactly what the expression answers. Go checks this
/// with a fuzzer; here the same seed text runs, plus deterministic mutations
/// of it. The seed is built from the table's own features, so a new feature
/// is checked without editing the test: a sample match, the same match after
/// a letter (word boundary), after a space, in lower and upper case.
#[test]
fn index_agrees_with_regexp() {
    let mut seeds = Vec::new();
    let mut all = Vec::new();
    for entry in COMPILED.iter() {
        for f in &entry.body {
            let ex = example_of(&f.go);
            all.push(ex.clone());
            for seed in [
                ex.clone(),
                format!("x{ex}"),
                format!(" {ex}"),
                ex.to_lowercase(),
                ex.to_uppercase(),
            ] {
                seeds.push(seed);
            }
        }
    }
    seeds.push(all.join(" "));
    seeds.push(super::match_test::bench_page(20 << 10));

    let mut rng = Rng(42);
    let base = seeds.len();
    for _ in 0..4000 {
        // Glue pieces of two seeds and cut the result at a random place: a
        // feature then lands next to a letter, a digit, a dash or nothing
        let a = &seeds[rng.below(base)];
        let b = &seeds[rng.below(base)];
        let glue = ["", "a", "_", "-", "1", "я", " ", "<", "/"][rng.below(9)];
        let mut s = format!("{a}{glue}{b}");
        let cut = rng.below(s.len() + 1);
        if s.is_char_boundary(cut) {
            s.truncate(cut.max(1).min(s.len()));
        }
        seeds.push(s);
    }

    for html in &seeds {
        // The automaton folds case in ASCII only, see Automaton
        if html.contains(['\u{17f}', '\u{212a}']) {
            continue;
        }
        let hits = SIGNATURE_INDEX.scan(html.as_bytes());
        for (i, entry) in COMPILED.iter().enumerate() {
            for (j, f) in entry.body.iter().enumerate() {
                let got = SIGNATURE_INDEX.plans[i][j].matches(&hits, &f.re, html);
                let want = f.re.is_match(html);
                assert_eq!(
                    got, want,
                    "{}, feature {:?} on {:.200?}: index {got}, expression {want}",
                    entry.name, f.go, html
                );
            }
        }
    }
}

/// A sample text the expression matches: the first alternative, the minimum
/// of a repetition, a character of a class.
fn example_of(pattern: &str) -> String {
    let tree = ast::parse::Parser::new().parse(pattern).unwrap();
    let mut out = String::new();
    example_into(&tree, pattern, &mut out);
    out
}

fn example_into(node: &Ast, pattern: &str, out: &mut String) {
    match node {
        Ast::Literal(l) => out.push(l.c),
        Ast::Dot(_) => out.push('x'),
        Ast::ClassPerl(c) => out.push(match c.kind {
            ast::ClassPerlKind::Digit => '0',
            ast::ClassPerlKind::Space => ' ',
            ast::ClassPerlKind::Word => 'a',
        }),
        Ast::ClassBracketed(c) => {
            let class = Regex::new(&format!(
                "^{}$",
                re2::translate(&pattern[c.span.start.offset..c.span.end.offset])
            ))
            .unwrap();
            let pick = "a "
                .chars()
                .chain('a'..='z')
                .chain('A'..='Z')
                .chain('0'..='9')
                .chain("-_./\"'=(?&".chars());
            out.push(
                pick.into_iter()
                    .find(|ch| class.is_match(&ch.to_string()))
                    .expect("a class with a printable member"),
            );
        }
        Ast::Repetition(r) => {
            let times = match &r.op.kind {
                ast::RepetitionKind::ZeroOrOne | ast::RepetitionKind::ZeroOrMore => 0,
                ast::RepetitionKind::OneOrMore => 1,
                ast::RepetitionKind::Range(range) => match *range {
                    ast::RepetitionRange::Exactly(n)
                    | ast::RepetitionRange::AtLeast(n)
                    | ast::RepetitionRange::Bounded(n, _) => n,
                },
            };
            for _ in 0..times {
                example_into(&r.ast, pattern, out);
            }
        }
        Ast::Group(g) => example_into(&g.ast, pattern, out),
        Ast::Alternation(a) => example_into(&a.asts[0], pattern, out),
        Ast::Concat(c) => c.asts.iter().for_each(|n| example_into(n, pattern, out)),
        // Anchors, boundaries and flags need no text of their own
        _ => {}
    }
}

/// A page of an ordinary WordPress site of about 100 KB: markup, Russian text,
/// scripts and links, among which features are rare.
pub(crate) fn bench_page(size: usize) -> String {
    let head = concat!(
        r#"<!doctype html><html lang="ru"><head><meta charset="utf-8">"#,
        r#"<meta name="generator" content="WordPress 6.6.2"><title>Окна и двери в Калининграде</title>"#,
        r#"<link rel="stylesheet" href="/wp-content/themes/astra/style.css?ver=4.1">"#,
        r#"<script src="/wp-includes/js/jquery/jquery.min.js"></script></head><body class="home page-template">"#,
    );
    let block = concat!(
        r#"<div class="wp-block-group container"><section class="catalog__item" data-id="1842">"#,
        r#"<a href="/catalog/derevo-alyuminievaya-sistema/okno-gemini-68/" class="catalog__link">"#,
        r#"<img src="/wp-content/uploads/2026/08/okno-gemini-68-300x200.jpg" alt="Окно Gemini 68" loading="lazy""#,
        r#" width="300" height="200"></a>"#,
        r#"<h3 class="catalog__title">Деревоалюминиевое окно Gemini 68</h3>"#,
        r#"<p class="catalog__text">Профиль из клеёного бруса сосны, наружная облицовка алюминием, "#,
        r#"двухкамерный стеклопакет 4-16-4-16-4 с энергосберегающим покрытием. "#,
        r#"Цена от 38 400 ₽ за квадратный метр.</p>"#,
        r#"<button type="button" class="btn btn--primary js-order" data-product="okno-gemini-68">"#,
        r#"Рассчитать стоимость</button>"#,
        "</section></div>\n",
    );
    let mut s = String::from(head);
    while s.len() < size {
        s.push_str(block);
    }
    s.push_str(r#"<script>var ajaxurl="/wp-admin/admin-ajax.php";</script></body></html>"#);
    s
}

/// RE2's Perl classes are ASCII, and the translation keeps them so: a Cyrillic
/// letter before a feature is a word boundary, a no-break space and a vertical
/// tab are not `\s`, an Arabic-Indic digit is not `\d`.
#[test]
fn perl_classes_keep_their_ascii_meaning() {
    let re = |p: &str| Regex::new(&re2::translate(p)).unwrap();
    assert!(re(r"(?i)\bevo").is_match("яevo"));
    assert!(!re(r"(?i)\bevo").is_match("derevo"));
    assert!(re(r"(?i)\bevo").is_match("evo"));
    assert!(!re(r"a\sb").is_match("a\u{a0}b"));
    assert!(!re(r"a\sb").is_match("a\u{b}b"));
    assert!(re(r"a\sb").is_match("a\tb"));
    assert!(!re(r"\d").is_match("\u{663}"));
    assert!(re(r#"[^"'\s]x"#).is_match("ax"));
    assert!(!re(r#"[^"'\s]x"#).is_match(" x"));
    assert!(re(r"\[\[\+").is_match("[[+"));
    assert!(re(r"[]a]").is_match("]"));
    assert_eq!(re2::translate(r"[[:alpha:]\d]"), "[[:alpha:]0-9]");
}

/// Go reads each byte that does not start valid UTF-8 as one U+FFFD; a page in
/// windows-1251 keeps its length in characters.
#[test]
fn go_string_replaces_each_invalid_byte() {
    // "Окно" in windows-1251, then a truncated sequence that from_utf8_lossy
    // would merge into one replacement
    let bytes = [0xce, 0xea, 0xed, 0xee, b' ', 0xe0, 0xa0, b'x'];
    assert_eq!(
        go_string(&bytes),
        "\u{fffd}\u{fffd}\u{fffd}\u{fffd} \u{fffd}\u{fffd}x"
    );
    assert_eq!(go_string(b"plain"), "plain");
}

/// Every literal plan is the feature's own text, and every piece the automaton
/// filters by is a piece of the feature. The number of plans of each kind is
/// the one Go's parser gives for the same table (294 literals after `\b`, 203
/// bare literals, 357 patterns on 22.11.2026), so a change of the parser that
/// moved a feature between kinds shows up; a new feature moves the numbers
/// along with the table.
#[test]
fn plans_follow_the_features() {
    let mut kinds: HashMap<&str, usize> = HashMap::new();
    for (i, entry) in COMPILED.iter().enumerate() {
        for (j, f) in entry.body.iter().enumerate() {
            let p = &SIGNATURE_INDEX.plans[i][j];
            let src = f.go.trim_start_matches("(?i)");
            let kind = if let Some(id) = p.literal {
                let plain = unescape(src.trim_start_matches(r"\b")).to_ascii_lowercase();
                assert_eq!(
                    SIGNATURE_INDEX.texts[id], plain,
                    "{}: literal of {src:?}",
                    entry.name
                );
                assert_eq!(
                    p.boundary,
                    src.starts_with(r"\b"),
                    "{}: boundary of {src:?}",
                    entry.name
                );
                if p.boundary {
                    "boundary literal"
                } else {
                    "literal"
                }
            } else {
                for &fid in &p.factors {
                    let piece = &SIGNATURE_INDEX.texts[fid];
                    assert!(
                        unescape(src).to_ascii_lowercase().contains(piece.as_str()),
                        "{}: piece {piece:?} is not in {src:?}",
                        entry.name
                    );
                }
                "pattern"
            };
            *kinds.entry(kind).or_default() += 1;
        }
    }
    let want = HashMap::from([
        ("boundary literal", 294),
        ("literal", 203),
        ("pattern", 357),
    ]);
    assert_eq!(kinds, want);
}

/// The text of an escaped literal: `\t` is a tab, `\.` a dot.
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => match chars.next() {
                Some('t') => out.push('\t'),
                Some(e) => out.push(e),
                None => {}
            },
            (c, false) => out.push(c),
        }
    }
    out
}
