// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

package cmslens

import (
	"net/netip"
	"net/url"
	"regexp"
	"strings"

	"github.com/profax/cmslens/internal/punycode"
)

// hostnameRe checks the ASCII form of the host: a name typed in Cyrillic
// reaches it as punycode, so the TLD may look like xn--p1ai (.рф).
var hostnameRe = regexp.MustCompile(`^[a-z0-9](?:[a-z0-9._-]*[a-z0-9])?\.(?:xn--[a-z0-9-]+|[a-z]{2,})$`)

/*
NormalizeDomain turns user input (a URL, a bare host, with or without www) into
a host name to scan. A name in Cyrillic or another script comes back as
punycode: президент.рф becomes xn--d1abbgf6aiiy.xn--p1ai. IP literals are rejected: there is no reason to scan by
address bypassing DNS, and an address is the easiest way to reach an internal
network (the fetcher also checks the address it actually connects to, not just
the name typed in).
*/
func NormalizeDomain(input string) (string, bool) {
	str := strings.ToLower(strings.TrimSpace(input))
	if str == "" {
		return "", false
	}
	str = strings.TrimPrefix(str, "https://")
	str = strings.TrimPrefix(str, "http://")
	str = strings.TrimPrefix(str, "www.")
	str, _, _ = strings.Cut(str, "/")
	str, _, _ = strings.Cut(str, "?")
	str, _, _ = strings.Cut(str, "#")

	parsed, err := url.Parse("http://" + str)
	if err != nil {
		return "", false
	}
	host := parsed.Hostname()

	if host == "" || strings.HasPrefix(host, "[") {
		return "", false
	}
	// url.Parse leaves the host as typed; DNS and the checks below need ASCII
	host, err = punycode.ToASCII(host)
	if err != nil {
		return "", false
	}
	if _, err := netip.ParseAddr(host); err == nil {
		return "", false
	}

	if hostnameRe.MatchString(host) {
		return host, true
	}
	return "", false
}
