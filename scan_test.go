// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

package cmslens

import "testing"

// The ScraperAPI key travels in the query of the request, and the request went
// over plain http: anyone on the path between the scanner and ScraperAPI could
// read the key and spend its credits.
func TestScraperAPIURLUsesHTTPS(t *testing.T) {
	t.Parallel()

	cases := []struct {
		opts Options
		want string
	}{
		{
			Options{ScraperAPIKey: "k&y"},
			"https://api.scraperapi.com?api_key=k%26y&url=https%3A%2F%2Fexample.com%2F%3Fa%3D1",
		},
		{
			Options{ScraperAPIKey: "key", ScraperCountry: "de"},
			"https://api.scraperapi.com?api_key=key&url=https%3A%2F%2Fexample.com%2F%3Fa%3D1&country_code=de",
		},
	}
	for _, c := range cases {
		if got := scraperAPIURL(c.opts, "https://example.com/?a=1"); got != c.want {
			t.Errorf("scraperAPIURL(%+v)\n got %s\nwant %s", c.opts, got, c.want)
		}
	}
}
