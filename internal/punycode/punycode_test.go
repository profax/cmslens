// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

package punycode

import (
	"errors"
	"strings"
	"testing"
)

// Samples from RFC 3492, section 7.1, and names whose punycode is well known.
func TestToASCII(t *testing.T) {
	t.Parallel()

	cases := map[string]string{
		"example.com":      "example.com",
		"президент.рф":     "xn--d1abbgf6aiiy.xn--p1ai",
		"ПРИМЕР.ИСПЫТАНИЕ": "xn--e1afmkfd.xn--80akhbyknj4f",
		"bücher.de":        "xn--bcher-kva.de",
		"münchen.de":       "xn--mnchen-3ya.de",
		"почемужеонинеговорятпорусски": "xn--b1abfaaepdrnnbgefbadotcwatmq2g4l",
		"ليهمابتكلموشعربي؟":            "xn--egbpdaj6bu4bxfgehfvwxn",
		"pročprostěnemluvíčesky":       "xn--proprostnemluvesky-uyb24dma41a",
	}
	checkToASCII(t, cases)
}

// The Chinese and Japanese samples of RFC 3492, kept apart for their linter
// notes.
func TestToASCIIHan(t *testing.T) {
	t.Parallel()

	checkToASCII(t, map[string]string{
		"他们为什么不说中文": "xn--ihqwcrb4cv8a8dqg056pqjye", //nolint:gosmopolitan // the sample of RFC 3492
		"3年b組金八先生":  "xn--3b-ww4c5e180e575a65lsy2b", //nolint:gosmopolitan // the sample of RFC 3492
	})
}

func checkToASCII(t *testing.T, cases map[string]string) {
	t.Helper()
	for input, want := range cases {
		got, err := ToASCII(input)
		if err != nil || got != want {
			t.Errorf("ToASCII(%q) = %q, %v; want %q", input, got, err, want)
		}
	}
}

func TestToASCIIRejectsALabelTooLongForDNS(t *testing.T) {
	t.Parallel()

	if got, err := ToASCII(strings.Repeat("я", 60) + ".рф"); !errors.Is(err, ErrLabel) {
		t.Errorf("a label of 60 Cyrillic letters passed as %q, error %v", got, err)
	}
}
