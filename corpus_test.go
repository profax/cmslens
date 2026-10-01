package cmslens

import (
	"bufio"
	"net/http"
	"net/textproto"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

/*
Runs the table against responses of live sites: this is how a new batch of
features is checked for false positives. Markup of other people's sites is not
stored in the repository; testdata/corpus.sh downloads it into the directory
named by CMSLENS_CORPUS. testdata/corpus.tsv gives the expected answer per
domain; domains without one are only printed.
*/
func TestCorpus(t *testing.T) {
	dir := os.Getenv("CMSLENS_CORPUS")
	if dir == "" {
		t.Skip("CMSLENS_CORPUS is not set")
	}
	expect := readExpect(t, filepath.Join("testdata", "corpus.tsv"))
	pages, err := filepath.Glob(filepath.Join(dir, "*.html"))
	if err != nil {
		t.Fatal(err)
	}
	for _, page := range pages {
		domain := strings.TrimSuffix(filepath.Base(page), ".html")
		body, err := os.ReadFile(page)
		if err != nil {
			t.Fatal(err)
		}
		if len(body) == 0 {
			continue
		}
		raw, err := os.ReadFile(filepath.Join(dir, domain+".headers"))
		if err != nil {
			t.Fatal(err)
		}
		scores := scoreResponse(string(body), lastHeaderBlock(string(raw)))
		got := SelectWinners(scores)
		if len(got) == 0 {
			got = []string{"-"}
		}

		want, ok := expect[domain]
		switch {
		case !ok:
			t.Logf("%-32s %-28s %v", domain, strings.Join(got, ","), scores)
		case !sameNames(got, want):
			t.Errorf("%s: got %s, want %s; scores %v", domain, strings.Join(got, ","), strings.Join(want, ","), scores)
		}
	}
}

func readExpect(t *testing.T, path string) map[string][]string {
	out := map[string][]string{}
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	for _, line := range strings.Split(string(raw), "\n") {
		domain, names, _ := strings.Cut(line, "\t")
		if names != "" && !strings.HasPrefix(domain, "#") {
			out[domain] = strings.Split(names, ",")
		}
	}
	return out
}

func sameNames(a, b []string) bool {
	a, b = slices.Clone(a), slices.Clone(b)
	slices.Sort(a)
	slices.Sort(b)
	return slices.Equal(a, b)
}

// lastHeaderBlock takes the headers of the final response: curl -L writes the
// headers of every redirect hop into the file one after another.
func lastHeaderBlock(raw string) http.Header {
	blocks := strings.Split(strings.TrimSpace(raw), "\r\n\r\n")
	block := blocks[len(blocks)-1]
	_, rest, _ := strings.Cut(block, "\r\n")
	h, _ := textproto.NewReader(bufio.NewReader(strings.NewReader(rest + "\r\n\r\n"))).ReadMIMEHeader()
	return http.Header(h)
}
