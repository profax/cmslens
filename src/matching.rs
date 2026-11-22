// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! Body features are searched in a single automaton pass rather than one
//! expression after another; otherwise the cost of a check grows linearly with
//! the number of platforms in the table. A literal feature is decided by the
//! automaton alone. For a pattern, the automaton looks for its required
//! pieces, and the expression runs only on a page where all of them were found.

use std::sync::LazyLock;

use aho_corasick::{AhoCorasick, MatchKind};
use regex::Regex;
use regex_syntax::ast::{self, Ast};

use crate::signatures::{COMPILED, CompiledEntry};

/// A piece shorter than three bytes occurs on every page and filters nothing
/// out.
pub(crate) const MIN_FACTOR_LEN: usize = 3;

pub(crate) static SIGNATURE_INDEX: LazyLock<BodyIndex> =
    LazyLock::new(|| BodyIndex::new(&COMPILED));

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeaturePlan {
    /// Index of the literal in the automaton if the feature is a pure literal.
    pub(crate) literal: Option<usize>,
    pub(crate) boundary: bool,
    pub(crate) factors: Vec<usize>,
}

#[derive(Debug)]
pub(crate) struct BodyIndex {
    pub(crate) ac: Automaton,
    pub(crate) texts: Vec<String>,
    /// `[table entry][body feature]`
    pub(crate) plans: Vec<Vec<FeaturePlan>>,
}

#[derive(Debug)]
pub(crate) struct BodyHits {
    seen: Vec<bool>,
    /// At least one occurrence sits on a word boundary, as a leading `\b`
    /// requires.
    bounded: Vec<bool>,
}

impl BodyIndex {
    pub(crate) fn new(table: &[CompiledEntry]) -> Self {
        let mut texts: Vec<String> = Vec::new();
        let mut ids = std::collections::HashMap::new();
        let mut intern = |s: &str| -> usize {
            let s = s.to_ascii_lowercase();
            *ids.entry(s.clone()).or_insert_with(|| {
                texts.push(s);
                texts.len() - 1
            })
        };
        let plans = table
            .iter()
            .map(|entry| {
                entry
                    .body
                    .iter()
                    .map(|f| plan_feature(&f.go, &mut intern))
                    .collect()
            })
            .collect();
        let ac = Automaton::new(&texts);
        Self { ac, texts, plans }
    }

    pub(crate) fn scan(&self, html: &[u8]) -> BodyHits {
        let mut h = BodyHits {
            seen: vec![false; self.texts.len()],
            bounded: vec![false; self.texts.len()],
        };
        self.ac.scan(html, |id, end| {
            h.seen[id] = true;
            if h.bounded[id] {
                return;
            }
            let text = self.texts[id].as_bytes();
            let start = end + 1 - text.len();
            let prev_word = start > 0 && is_word_byte(html[start - 1]);
            h.bounded[id] = prev_word != is_word_byte(text[0]);
        });
        h
    }
}

impl FeaturePlan {
    /// `html` is the body as raw bytes for the automaton's answer and as text
    /// for the expression; the two are the same page.
    pub(crate) fn matches(&self, h: &BodyHits, re: &Regex, html: &str) -> bool {
        if let Some(id) = self.literal {
            return if self.boundary {
                h.bounded[id]
            } else {
                h.seen[id]
            };
        }
        if self.factors.iter().any(|&f| !h.seen[f]) {
            return false;
        }
        re.is_match(html)
    }
}

/// A piece of the top-level concatenation, as Go's `regexp/syntax` builds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    WordBoundary,
    /// A run of literal characters under the same case folding: Go's parser
    /// merges them into one literal node.
    Literal {
        text: String,
        fold: bool,
    },
    Other,
}

/// Splits a feature into the pieces of its top-level concatenation, the same
/// pieces Go's parser gives: the plan, and with it the automaton's share of
/// the answer, is then the one Go had.
pub(crate) fn feature_nodes(pattern: &str) -> Vec<Node> {
    let tree = ast::parse::Parser::new()
        .parse(pattern)
        .unwrap_or_else(|e| panic!("feature {pattern:?} does not parse: {e}"));
    let items: &[Ast] = match &tree {
        Ast::Concat(c) => &c.asts,
        other => std::slice::from_ref(other),
    };
    let mut fold = false;
    let mut nodes: Vec<Node> = Vec::new();
    for item in items {
        let node = match item {
            Ast::Flags(f) => {
                if let Some(on) = f.flags.flag_state(ast::Flag::CaseInsensitive) {
                    fold = on;
                }
                continue;
            }
            Ast::Literal(l) => Node::Literal {
                text: l.c.to_string(),
                fold,
            },
            Ast::Assertion(a) if a.kind == ast::AssertionKind::WordBoundary => Node::WordBoundary,
            Ast::ClassBracketed(c) => bracket_literal(c, fold).unwrap_or(Node::Other),
            _ => Node::Other,
        };
        // Go merges a literal into the previous one with the same folding
        if let (
            Node::Literal { text, fold: f },
            Some(Node::Literal {
                text: prev,
                fold: pf,
            }),
        ) = (&node, nodes.last_mut())
            && f == pf
        {
            prev.push_str(text);
            continue;
        }
        nodes.push(node);
    }
    if nodes.is_empty() {
        // Go's parser gives an empty match, which is neither literal nor piece
        nodes.push(Node::Other);
    }
    nodes
}

/// Go's parser turns a class of one rune (`[.]`) into a literal without case
/// folding, and a class of a case pair (`[Aa]`) into a folded one.
fn bracket_literal(c: &ast::ClassBracketed, fold: bool) -> Option<Node> {
    if c.negated {
        return None;
    }
    let one = |item: &ast::ClassSetItem| match item {
        ast::ClassSetItem::Literal(l) => Some(l.c),
        _ => None,
    };
    let ast::ClassSet::Item(item) = &c.kind else {
        return None;
    };
    match item {
        ast::ClassSetItem::Literal(l) if !fold || !has_case(l.c) => Some(Node::Literal {
            text: l.c.to_string(),
            fold: false,
        }),
        // Folded, `[a]` is the pair [Aa]; `[k]` and `[s]` gain the Kelvin sign
        // and "ſ" and stay a class
        ast::ClassSetItem::Literal(l) if !matches!(l.c, 'k' | 'K' | 's' | 'S') => {
            Some(Node::Literal {
                text: l.c.to_string(),
                fold: true,
            })
        }
        ast::ClassSetItem::Union(u) if u.items.len() == 2 => {
            let (a, b) = (one(&u.items[0])?, one(&u.items[1])?);
            let pair =
                a != b && a.to_lowercase().eq(b.to_lowercase()) && a.is_ascii() && b.is_ascii();
            pair.then(|| Node::Literal {
                text: a.to_string(),
                fold: true,
            })
        }
        _ => None,
    }
}

fn has_case(c: char) -> bool {
    c.to_lowercase().ne(c.to_uppercase())
}

pub(crate) fn plan_feature(pattern: &str, intern: &mut impl FnMut(&str) -> usize) -> FeaturePlan {
    let mut nodes = feature_nodes(pattern);
    let mut plan = FeaturePlan {
        literal: None,
        boundary: false,
        factors: Vec::new(),
    };
    if nodes.len() == 2 && nodes[0] == Node::WordBoundary {
        plan.boundary = true;
        nodes.remove(0);
    }
    // Without (?i) a literal is not handed to the automaton whole: the
    // automaton always folds case
    if let [Node::Literal { text, fold: true }] = nodes.as_slice()
        && text.is_ascii()
    {
        plan.literal = Some(intern(text));
        return plan;
    }

    plan.boundary = false;
    for n in &nodes {
        if let Node::Literal { text, .. } = n
            && text.is_ascii()
            && text.len() >= MIN_FACTOR_LEN
        {
            plan.factors.push(intern(text));
        }
    }
    plan
}

/// Mirrors RE2's `\b`: word boundaries there are ASCII only.
pub(crate) fn is_word_byte(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphanumeric()
}

/// Every occurrence of every literal, overlapping ones and literals inside
/// other literals included, with ASCII case folded on both sides. Case is
/// folded in ASCII only, while RE2's `(?i)` also folds "ſ" to "s" and the
/// Kelvin sign to "k"; neither shows up in real markup.
#[derive(Debug)]
pub(crate) struct Automaton(AhoCorasick);

impl Automaton {
    pub(crate) fn new(texts: &[String]) -> Self {
        let ac = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .match_kind(MatchKind::Standard)
            .build(texts)
            .expect("the literals of the table fit the automaton");
        Self(ac)
    }

    /// Calls `emit` with the literal's index and the index of its last byte.
    pub(crate) fn scan(&self, data: &[u8], mut emit: impl FnMut(usize, usize)) {
        for m in self.0.find_overlapping_iter(data) {
            emit(m.pattern().as_usize(), m.end() - 1);
        }
    }
}
