/*
False detections taken from live sites. Each test keeps one case: what exactly a
feature fragment matched in someone else's text, and what the answer should be
instead. Markup is trimmed to the relevant part; full responses are not stored
in the repository.
*/
package cmslens

import (
	"net/http"
	"regexp"
	"strings"
	"testing"
)

// www.tbm.ru came out as "Evolution CMS and Nuxt.js": the feature `evo-`
// matched inside the transliteration "derevo-alyuminievaya", and there is no
// trace of Evolution on the site. Another service on the same domain came out
// as NGCMS, which is not there either.
func TestTransliterationDoesNotNameEvolution(t *testing.T) {
	html := `<html><head><script>window.__NUXT__=(function(a){return {data:[{}]}})()</script></head>` +
		`<body><a href="/derevo-alyuminievaya-sistema/derevo-alyuminievaya-sistema-gemini/">Gemini</a>` +
		`<a href="https://ml.rbc.ru/arctic26s-c2-kmiva-dizanalevo-m">РБК</a>` +
		`<div data-v-7f3a1c2b>Двери</div></body></html>`

	names := SelectWinners(scoreResponse(html, http.Header{"X-Powered-By": []string{"Nuxt"}}))

	assertNames(t, names, []string{"Nuxt.js"})
}

// www.kommersant.ru was named Jekyll because of `_site/` matching inside
// someone else's image path. `_site/` is Jekyll's build directory and never
// appears in served URLs, so the feature was removed rather than anchored.
func TestBuildDirectoryPathDoesNotNameJekyll(t *testing.T) {
	html := `<html><body><img src="/Issues.photo2/WEEKEND_Online_Site/2026/08/28/KMO_17838.jpg"></body></html>`

	if names := SelectWinners(scoreResponse(html, http.Header{})); len(names) != 0 {
		t.Errorf("this page runs no engine, got %v", names)
	}
}

// The `csrf-param` and `csrf-token` pair is written by Rails, Yii, InstantCMS
// and hand-made backends: www.rbc.ru was named Rails one day and Laravel the
// next. Only the value `authenticity_token` is specific to Rails.
func TestSharedCsrfTokenNamesNoEngine(t *testing.T) {
	shared := `<html><head><meta name="csrf-token" content="9d1f"><meta name="csrf-param" content="csrf_token"></head></html>`
	assertNames(t, SelectWinners(scoreResponse(shared, http.Header{})), nil)

	rails := `<html><head><meta name="csrf-param" content="authenticity_token"><meta name="csrf-token" content="9d1f"></head></html>`
	assertNames(t, SelectWinners(scoreResponse(rails, http.Header{})), []string{"Ruby on Rails"})
}

/*
A rule instead of a list: a short fragment feature must start at a word
boundary. This class of bug is invisible by eye: a string looks specific right
until the day someone's slug ends with it, and the site gets an engine it does
not run. Features with a delimiter (`/`, `<`, `"`, `=`, `.`) are exempt: the
delimiter is the boundary.
*/
var bareFragment = regexp.MustCompile(`^[A-Za-z0-9_-]+$`)

func TestBareFragmentsAreWordAnchored(t *testing.T) {
	for _, entry := range cmsSignatures {
		for _, item := range entry.sig.body {
			src := strings.TrimPrefix(item.re.String(), "(?i)")
			if strings.HasPrefix(src, `\b`) {
				continue
			}
			if bareFragment.MatchString(src) {
				t.Errorf("%s: feature %q has no leading \\b and will match inside other words", entry.name, src)
			}
		}
	}
}
