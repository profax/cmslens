package cmslens

import "testing"

func TestNormalizeDomainKeepsTheHost(t *testing.T) {
	t.Parallel()

	cases := map[string]string{
		"example.com":                       "example.com",
		"  Example.COM  ":                   "example.com",
		"www.example.com":                   "example.com",
		"https://www.example.com/a?b#c":     "example.com",
		"http://example.com:8080/path":      "example.com",
		"example.com?x=1":                   "example.com",
		"example.com#top":                   "example.com",
		"http://user@example.com":           "example.com",
		"shop.example.co.uk":                "shop.example.co.uk",
		"xn--d1abbgf6aiiy.xn--p1ai":         "xn--d1abbgf6aiiy.xn--p1ai",
		"HTTPS://XN--D1ABBGF6AIIY.XN--P1AI": "xn--d1abbgf6aiiy.xn--p1ai",
	}
	for input, want := range cases {
		got, ok := NormalizeDomain(input)
		if !ok || got != want {
			t.Errorf("NormalizeDomain(%q) = %q, %v; want %q", input, got, ok, want)
		}
	}
}

// An address instead of a name is the shortest way into an internal network,
// and a name without a public suffix is not a site on the internet.
func TestNormalizeDomainRejectsWhatIsNotADomain(t *testing.T) {
	t.Parallel()

	for _, input := range []string{
		"", "   ", "www.",
		"localhost", "intranet", "a.b",
		"1.2.3.4", "https://192.168.1.1/admin", "[::1]", "http://[::1]:8080/",
		"-bad.example.com", "ex ample.com", "example.c0m",
	} {
		if got, ok := NormalizeDomain(input); ok {
			t.Errorf("NormalizeDomain(%q) = %q, want a rejection", input, got)
		}
	}
}
