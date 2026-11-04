// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

// Package punycode turns a host name written in any script into the ASCII
// form DNS knows (RFC 3492, with the xn-- prefix of RFC 5890).
//
// It is the encoding alone, without the IDNA mapping table: letters are
// lower-cased and kept as typed. That covers names people type in Cyrillic
// and other alphabets, and it never turns a look-alike character into an
// ASCII one, so a full-width "１２７" cannot become 127.
package punycode

import (
	"errors"
	"strings"
	"unicode/utf8"
)

// Bootstring parameters for punycode, RFC 3492 section 5.
const (
	base        = 36
	tMin        = 1
	tMax        = 26
	skew        = 38
	damp        = 700
	initialBias = 72
	initialN    = 0x80
	// The digit d of a number is digits[d].
	digits = "abcdefghijklmnopqrstuvwxyz0123456789"
)

const (
	acePrefix   = "xn--"
	maxLabelLen = 63
)

// ErrLabel means a label cannot become a DNS label.
var ErrLabel = errors.New("label does not fit DNS")

// ToASCII lower-cases the host and encodes every label that is not ASCII.
func ToASCII(host string) (string, error) {
	labels := strings.Split(strings.ToLower(host), ".")
	for i, label := range labels {
		if isASCII(label) {
			continue
		}
		encoded := acePrefix + encode([]rune(label))
		if len(encoded) > maxLabelLen {
			return "", ErrLabel
		}
		labels[i] = encoded
	}
	return strings.Join(labels, "."), nil
}

func isASCII(s string) bool {
	for i := range len(s) {
		if s[i] >= utf8.RuneSelf {
			return false
		}
	}
	return true
}

// encode is the encoding procedure of RFC 3492, section 6.3. Labels are at most
// a few dozen runes, far below the sizes where the arithmetic could overflow.
func encode(input []rune) string {
	var out strings.Builder
	for _, r := range input {
		if r < initialN {
			out.WriteRune(r)
		}
	}
	basic := out.Len()
	if basic > 0 {
		out.WriteByte('-')
	}

	n, delta, bias := rune(initialN), 0, initialBias
	for handled := basic; handled < len(input); {
		next := smallestFrom(input, n)
		delta += int(next-n) * (handled + 1)
		n = next
		for _, r := range input {
			if r < n {
				delta++
			}
			if r != n {
				continue
			}
			writeNumber(&out, delta, bias)
			bias = adapt(delta, handled+1, handled == basic)
			delta = 0
			handled++
		}
		delta++
		n++
	}
	return out.String()
}

// smallestFrom is the smallest code point in input that is at least n.
func smallestFrom(input []rune, n rune) rune {
	smallest := rune(utf8.MaxRune)
	for _, r := range input {
		if r >= n && r < smallest {
			smallest = r
		}
	}
	return smallest
}

// writeNumber writes q as a generalized variable-length integer.
func writeNumber(out *strings.Builder, q, bias int) {
	for k := base; ; k += base {
		t := min(max(k-bias, tMin), tMax)
		if q < t {
			break
		}
		out.WriteByte(digits[t+(q-t)%(base-t)])
		q = (q - t) / (base - t)
	}
	out.WriteByte(digits[q])
}

func adapt(delta, points int, first bool) int {
	if first {
		delta /= damp
	} else {
		delta /= 2
	}
	delta += delta / points
	k := 0
	for delta > (base-tMin)*tMax/2 {
		delta /= base - tMin
		k += base
	}
	return k + (base-tMin+1)*delta/(delta+skew)
}
