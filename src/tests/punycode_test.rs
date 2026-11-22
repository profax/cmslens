// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use crate::punycode::{LabelError, to_ascii};

fn check(cases: &[(&str, &str)]) {
    for (input, want) in cases {
        assert_eq!(to_ascii(input).as_deref(), Ok(*want), "to_ascii({input:?})");
    }
}

/// Samples from RFC 3492, section 7.1, and names whose punycode is well known.
#[test]
fn to_ascii_samples() {
    check(&[
        ("example.com", "example.com"),
        ("президент.рф", "xn--d1abbgf6aiiy.xn--p1ai"),
        ("ПРИМЕР.ИСПЫТАНИЕ", "xn--e1afmkfd.xn--80akhbyknj4f"),
        ("bücher.de", "xn--bcher-kva.de"),
        ("münchen.de", "xn--mnchen-3ya.de"),
        (
            "почемужеонинеговорятпорусски",
            "xn--b1abfaaepdrnnbgefbadotcwatmq2g4l",
        ),
        ("ليهمابتكلموشعربي؟", "xn--egbpdaj6bu4bxfgehfvwxn"),
        (
            "pročprostěnemluvíčesky",
            "xn--proprostnemluvesky-uyb24dma41a",
        ),
    ]);
}

/// The Chinese and Japanese samples of RFC 3492.
#[test]
fn to_ascii_han() {
    check(&[
        ("他们为什么不说中文", "xn--ihqwcrb4cv8a8dqg056pqjye"),
        ("3年b組金八先生", "xn--3b-ww4c5e180e575a65lsy2b"),
    ]);
}

#[test]
fn to_ascii_rejects_a_label_too_long_for_dns() {
    let long = format!("{}.рф", "я".repeat(60));
    assert_eq!(to_ascii(&long), Err(LabelError));
}
