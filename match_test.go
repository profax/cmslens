package cmslens

import (
	"maps"
	"math/rand/v2"
	"net/http"
	"regexp/syntax"
	"strings"
	"testing"
)

// The feature table has almost no literals nested inside each other, so it does
// not exercise suffix chains. Literals over a three-letter alphabet produce
// them at once.
func TestAutomatonFindsEveryOccurrence(t *testing.T) {
	rng := rand.New(rand.NewPCG(1, 2))
	word := func(n int) string {
		b := make([]byte, n)
		for i := range b {
			b[i] = "abcABC"[rng.IntN(6)]
		}
		return string(b)
	}
	for round := 0; round < 200; round++ {
		ids := map[string]bool{}
		var texts []string
		for len(texts) < 8 {
			if s := strings.ToLower(word(1 + rng.IntN(4))); !ids[s] {
				ids[s] = true
				texts = append(texts, s)
			}
		}
		data := word(300)

		want := map[[2]int]bool{}
		lower := strings.ToLower(data)
		for id, text := range texts {
			for end := len(text) - 1; end < len(lower); end++ {
				if lower[end+1-len(text):end+1] == text {
					want[[2]int{id, end}] = true
				}
			}
		}
		got := map[[2]int]bool{}
		newAutomaton(texts).scan(data, func(id int32, end int) { got[[2]int{int(id), end}] = true })

		if !maps.Equal(got, want) {
			t.Fatalf("literals %q in %q: found %d occurrences, want %d", texts, data, len(got), len(want))
		}
	}
}

// A feature without a literal runs its expression over every page, and the
// number of such features would grow back to the number of platforms.
func TestEveryBodyFeatureNarrowsByLiteral(t *testing.T) {
	for i, entry := range cmsSignatures {
		for j, item := range entry.sig.body {
			if p := signatureIndex.plans[i][j]; p.literal < 0 && len(p.factors) == 0 {
				t.Errorf("%s: feature %q has no literal piece of %d bytes or more", entry.name, item.re, minFactorLen)
			}
		}
	}
}

/*
The index must answer exactly what the expression answers. The seed text is
built from the table's own features, so a new feature is checked without editing
the test: a sample match, the same match after a letter (word boundary), after a
space, and in lower case.
*/
func FuzzIndexAgreesWithRegexp(f *testing.F) {
	var all []string
	for _, entry := range cmsSignatures {
		for _, item := range entry.sig.body {
			tree, err := syntax.Parse(item.re.String(), syntax.Perl)
			if err != nil {
				f.Fatal(err)
			}
			ex := exampleOf(tree)
			all = append(all, ex)
			for _, seed := range []string{ex, "x" + ex, " " + ex, strings.ToLower(ex)} {
				f.Add(seed)
			}
		}
	}
	f.Add(strings.Join(all, " "))
	f.Add(benchPage(20 << 10))

	f.Fuzz(func(t *testing.T, html string) {
		// The automaton folds case in ASCII only, see automaton
		if strings.ContainsAny(html, "\u017f\u212a") {
			t.Skip()
		}
		hits := signatureIndex.scan(html)
		for i, entry := range cmsSignatures {
			for j, item := range entry.sig.body {
				got := signatureIndex.plans[i][j].matches(hits, item.re, html)
				if want := item.re.MatchString(html); got != want {
					t.Errorf("%s, feature %q on %.200q: index %v, expression %v", entry.name, item.re, html, got, want)
				}
			}
		}
	})
}

func exampleOf(re *syntax.Regexp) string {
	switch re.Op {
	case syntax.OpLiteral:
		return string(re.Rune)
	case syntax.OpCharClass:
		return string(classExample(re.Rune))
	case syntax.OpAnyChar, syntax.OpAnyCharNotNL:
		return "x"
	case syntax.OpCapture, syntax.OpPlus, syntax.OpAlternate:
		return exampleOf(re.Sub[0])
	case syntax.OpRepeat:
		return strings.Repeat(exampleOf(re.Sub[0]), re.Min)
	case syntax.OpConcat:
		var sb strings.Builder
		for _, sub := range re.Sub {
			sb.WriteString(exampleOf(sub))
		}
		return sb.String()
	}
	return ""
}

func classExample(ranges []rune) rune {
	for _, want := range []rune{'a', ' '} {
		for k := 0; k < len(ranges); k += 2 {
			if ranges[k] <= want && want <= ranges[k+1] {
				return want
			}
		}
	}
	return ranges[0]
}

// benchPage is a page of an ordinary WordPress site of about 100 KB: markup,
// Russian text, scripts and links, among which features are rare.
func benchPage(size int) string {
	head := `<!doctype html><html lang="ru"><head><meta charset="utf-8">` +
		`<meta name="generator" content="WordPress 6.6.2"><title>Окна и двери в Калининграде</title>` +
		`<link rel="stylesheet" href="/wp-content/themes/astra/style.css?ver=4.1">` +
		`<script src="/wp-includes/js/jquery/jquery.min.js"></script></head><body class="home page-template">`
	block := `<div class="wp-block-group container"><section class="catalog__item" data-id="1842">` +
		`<a href="/catalog/derevo-alyuminievaya-sistema/okno-gemini-68/" class="catalog__link">` +
		`<img src="/wp-content/uploads/2026/08/okno-gemini-68-300x200.jpg" alt="Окно Gemini 68" loading="lazy" width="300" height="200"></a>` +
		`<h3 class="catalog__title">Деревоалюминиевое окно Gemini 68</h3>` +
		`<p class="catalog__text">Профиль из клеёного бруса сосны, наружная облицовка алюминием, ` +
		`двухкамерный стеклопакет 4-16-4-16-4 с энергосберегающим покрытием. Цена от 38 400 ₽ за квадратный метр.</p>` +
		`<button type="button" class="btn btn--primary js-order" data-product="okno-gemini-68">Рассчитать стоимость</button>` +
		`</section></div>` + "\n"
	var sb strings.Builder
	sb.WriteString(head)
	for sb.Len() < size {
		sb.WriteString(block)
	}
	sb.WriteString(`<script>var ajaxurl="/wp-admin/admin-ajax.php";</script></body></html>`)
	return sb.String()
}

func BenchmarkScoreResponse(b *testing.B) {
	html := benchPage(100 << 10)
	headers := http.Header{"Set-Cookie": []string{"wordpress_test_cookie=WP%20Cookie%20check"}}
	b.SetBytes(int64(len(html)))
	for b.Loop() {
		scoreResponse(html, headers)
	}
}
