package cmslens

import (
	"net/http"
	"reflect"
	"testing"
)

func scores(pairs ...any) []CMSScore {
	out := make([]CMSScore, 0, len(pairs)/2)
	for i := 0; i < len(pairs); i += 2 {
		out = append(out, CMSScore{Name: pairs[i].(string), Score: float64(pairs[i+1].(int))})
	}
	return out
}

func assertNames(t *testing.T, got []string, want []string) {
	t.Helper()
	if len(want) == 0 && len(got) == 0 {
		return
	}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("got %v, want %v", got, want)
	}
}

func TestSelectWinnersCompetingBackends(t *testing.T) {
	assertNames(t, SelectWinners(scores("Joomla", 3, "Winter CMS", 1)), []string{"Joomla"})
	assertNames(t, SelectWinners(scores("Drupal", 2, "WordPress", 5, "Joomla", 1)), []string{"WordPress"})
	assertNames(t, SelectWinners(scores("MODX", 1, "Evolution CMS", 3)), []string{"Evolution CMS"})
	assertNames(t, SelectWinners(scores("October CMS", 2, "Winter CMS", 3)), []string{"Winter CMS"})
	assertNames(t, SelectWinners(scores("1C-Bitrix", 4, "WordPress", 2)), []string{"1C-Bitrix"})
	assertNames(t, SelectWinners(scores("Webasyst", 2, "Shop-Script", 4)), []string{"Shop-Script"})
}

func TestSelectWinnersExactlyOneBackend(t *testing.T) {
	result := SelectWinners(scores("WordPress", 2, "Joomla", 1, "Drupal", 3, "OpenCart", 1, "Bitrix", 1))
	backendCount := 0
	for _, name := range result {
		if !frontendLayer[name] {
			backendCount++
		}
	}
	if backendCount != 1 {
		t.Errorf("want exactly one backend engine, got %d in %v", backendCount, result)
	}
}

func TestSelectWinnersHeadlessPairs(t *testing.T) {
	cases := []struct {
		scores []CMSScore
		front  string
		back   string
	}{
		{scores("WordPress", 3, "Next.js", 2), "Next.js", "WordPress"},
		{scores("Astro", 3, "Contentful", 2), "Astro", "Contentful"},
		{scores("1C-Bitrix", 4, "Nuxt.js", 2), "Nuxt.js", "1C-Bitrix"},
	}
	for _, c := range cases {
		result := SelectWinners(c.scores)
		if len(result) != 2 {
			t.Errorf("%v: want 2 results, got %v", c.scores, result)
			continue
		}
		if !contains(result, c.front) || !contains(result, c.back) {
			t.Errorf("%v: want %s and %s, got %v", c.scores, c.front, c.back, result)
		}
	}
}

func contains(list []string, item string) bool {
	for _, v := range list {
		if v == item {
			return true
		}
	}
	return false
}

func TestSelectWinnersCompetingFrontends(t *testing.T) {
	assertNames(t, SelectWinners(scores("Next.js", 3, "React", 2)), []string{"Next.js"})
	assertNames(t, SelectWinners(scores("Gatsby", 2, "React", 1)), []string{"Gatsby"})
}

// The October and Winter theme path weighs 0.5 and appears on unrelated sites:
// on its own it gives no answer, and next to a real engine it does not stop
// that engine from winning
func TestSelectWinnersIgnoresBelowThreshold(t *testing.T) {
	half := []CMSScore{{Name: "October CMS", Score: 0.5}, {Name: "Winter CMS", Score: 0.5}}
	assertNames(t, SelectWinners(half), nil)
	withEngine := append(half, CMSScore{Name: "ePages", Score: 1})
	assertNames(t, SelectWinners(withEngine), []string{"ePages"})
}

func TestSelectWinnersEdgeCases(t *testing.T) {
	if got := SelectWinners(nil); len(got) != 0 {
		t.Errorf("empty input must give empty output, got %v", got)
	}
	assertNames(t, SelectWinners(scores("Joomla", 3)), []string{"Joomla"})
	assertNames(t, SelectWinners(scores("Astro", 2)), []string{"Astro"})

	result := SelectWinners(scores("WordPress", 3, "Next.js", 5))
	if len(result) != 2 || result[0] != "Next.js" || result[1] != "WordPress" {
		t.Errorf("want order [Next.js WordPress] by descending score, got %v", result)
	}

	max2 := SelectWinners(scores("WordPress", 5, "Joomla", 3, "Drupal", 2, "Next.js", 4, "Astro", 3, "Vue", 2))
	if len(max2) > 2 {
		t.Errorf("want at most 2 results, got %d: %v", len(max2), max2)
	}
}

func TestFrontendLayerMembership(t *testing.T) {
	expected := []string{"Next.js", "Nuxt.js", "Astro", "Svelte", "Gatsby", "React", "Vue", "Angular", "Vite"}
	for _, fw := range expected {
		if !frontendLayer[fw] {
			t.Errorf("frontendLayer must include %s", fw)
		}
	}
	notFrontend := []string{"WordPress", "Joomla", "1C-Bitrix", "Drupal", "Winter CMS", "October CMS", "MODX"}
	for _, cms := range notFrontend {
		if frontendLayer[cms] {
			t.Errorf("frontendLayer must not include %s (it is a backend)", cms)
		}
	}
}

// TestAllSignaturesCompile: the table has 556 signatures. The test keeps the
// number visible on the next edit of the table.
func TestAllSignaturesCompile(t *testing.T) {
	if len(cmsSignatures) != 556 {
		t.Errorf("want 556 signatures, got %d", len(cmsSignatures))
	}
	seen := map[string]bool{}
	for _, entry := range cmsSignatures {
		if seen[entry.name] {
			t.Errorf("duplicate CMS name: %s", entry.name)
		}
		seen[entry.name] = true
		if len(entry.sig.body) == 0 && len(entry.sig.headers) == 0 {
			t.Errorf("%s: signature without a single feature", entry.name)
		}
	}
}

// A real case: kitanasushi.ru answers with a Next.js header but is built on the
// StarterApp SaaS platform, whose CDN domains sit on every client page. While
// the platform was missing from the table, the answer was the framework alone,
// i.e. the presentation layer instead of the engine. Another service answered
// "VamShop" for the same site, though there is no trace of that CMS at all.
func TestStarterAppWinsOverFrontendLayer(t *testing.T) {
	html := `<html><head><link rel="preconnect" href="https://content.cdn.starterapp.ru">` +
		`<script src="/_next/static/chunks/main.js"></script></head><body>` +
		`<img src="https://cdn.starterapp.ru/logo.png"><script>fetch("https://api.starterapp.ru/v1/menu")</script></body></html>`

	names := SelectWinners(scoreResponse(html, http.Header{"X-Powered-By": []string{"Next.js"}}))

	if len(names) != 2 || names[0] != "StarterApp" || names[1] != "Next.js" {
		t.Fatalf("want the pair of platform and presentation layer [StarterApp Next.js], got %v", names)
	}
	for _, name := range names {
		if name == "VamShop" {
			t.Fatal("VamShop has nothing to do with this site: there is no trace of it in the response")
		}
	}
}
