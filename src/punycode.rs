// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! Turns a host name written in any script into the ASCII form DNS knows
//! (RFC 3492, with the xn-- prefix of RFC 5890).
//!
//! It is the encoding alone, without the IDNA mapping table: letters are
//! lower-cased and kept as typed. That covers names people type in Cyrillic
//! and other alphabets, and it never turns a look-alike character into an
//! ASCII one, so a full-width "１２７" cannot become 127.

use crate::gostr::to_lower;

// Bootstring parameters for punycode, RFC 3492 section 5.
const BASE: u32 = 36;
const T_MIN: u32 = 1;
const T_MAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 0x80;
/// The digit d of a number is `DIGITS[d]`.
const DIGITS: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";

const ACE_PREFIX: &str = "xn--";
const MAX_LABEL_LEN: usize = 63;

/// A label that cannot become a DNS label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LabelError;

/// Lower-cases the host and encodes every label that is not ASCII.
pub(crate) fn to_ascii(host: &str) -> Result<String, LabelError> {
    let lower = to_lower(host);
    let mut labels = Vec::new();
    for label in lower.split('.') {
        if label.is_ascii() {
            labels.push(label.to_owned());
            continue;
        }
        let encoded = format!(
            "{ACE_PREFIX}{}",
            encode(&label.chars().map(u32::from).collect::<Vec<_>>())
        );
        if encoded.len() > MAX_LABEL_LEN {
            return Err(LabelError);
        }
        labels.push(encoded);
    }
    Ok(labels.join("."))
}

/// The encoding procedure of RFC 3492, section 6.3. Labels are at most a few
/// dozen code points, far below the sizes where the arithmetic could overflow.
fn encode(input: &[u32]) -> String {
    let mut out = String::new();
    for &r in input {
        if r < INITIAL_N {
            out.push(char::from_u32(r).unwrap_or('?'));
        }
    }
    let basic = out.len();
    if basic > 0 {
        out.push('-');
    }

    let (mut n, mut delta, mut bias) = (INITIAL_N, 0u32, INITIAL_BIAS);
    let mut handled = basic;
    while handled < input.len() {
        let next = smallest_from(input, n);
        delta += (next - n) * count(handled + 1);
        n = next;
        for &r in input {
            if r < n {
                delta += 1;
            }
            if r != n {
                continue;
            }
            write_number(&mut out, delta, bias);
            bias = adapt(delta, count(handled + 1), handled == basic);
            delta = 0;
            handled += 1;
        }
        delta += 1;
        n += 1;
    }
    out
}

fn count(n: usize) -> u32 {
    u32::try_from(n).expect("a DNS label is far shorter than u32::MAX")
}

/// The smallest code point in input that is at least n.
fn smallest_from(input: &[u32], n: u32) -> u32 {
    input
        .iter()
        .copied()
        .filter(|&r| r >= n)
        .min()
        .unwrap_or(0x10_FFFF)
}

/// Writes q as a generalized variable-length integer.
fn write_number(out: &mut String, mut q: u32, bias: u32) {
    let mut k = BASE;
    loop {
        let t = k.saturating_sub(bias).clamp(T_MIN, T_MAX);
        if q < t {
            break;
        }
        out.push(char::from(DIGITS[(t + (q - t) % (BASE - t)) as usize]));
        q = (q - t) / (BASE - t);
        k += BASE;
    }
    out.push(char::from(DIGITS[q as usize]));
}

fn adapt(mut delta: u32, points: u32, first: bool) -> u32 {
    delta /= if first { DAMP } else { 2 };
    delta += delta / points;
    let mut k = 0;
    while delta > (BASE - T_MIN) * T_MAX / 2 {
        delta /= BASE - T_MIN;
        k += BASE;
    }
    k + (BASE - T_MIN + 1) * delta / (delta + SKEW)
}
