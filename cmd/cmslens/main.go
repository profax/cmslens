// Command cmslens tells which CMS or framework a website runs.
//
//	cmslens example.com wordpress.org
//	cmslens -json example.com
//	cmslens -file page.html
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"net/http"
	"os"
	"runtime/debug"
	"strings"
	"sync"
	"time"

	"github.com/profax/cmslens"
)

// Set by the release build; go install leaves it to the module version.
var version = "dev"

func init() {
	if info, ok := debug.ReadBuildInfo(); ok && version == "dev" && info.Main.Version != "" && info.Main.Version != "(devel)" {
		version = info.Main.Version
	}
}

// Sites are fetched in parallel, but not so many at once that a long list
// looks like an attack from this machine.
const parallel = 8

type line struct {
	Input  string         `json:"input"`
	Result cmslens.Result `json:"result"`
	Error  string         `json:"error,omitempty"`
}

func main() {
	asJSON := flag.Bool("json", false, "print one JSON object per site")
	file := flag.String("file", "", "detect from a saved HTML page instead of fetching (no headers are used)")
	timeout := flag.Duration("timeout", 60*time.Second, "time limit per site")
	scraperKey := flag.String("scraperapi-key", os.Getenv("SCRAPERAPI_KEY"), "retry blocked sites through ScraperAPI (default $SCRAPERAPI_KEY)")
	country := flag.String("country", "", "ScraperAPI country code, such as us or de")
	lang := flag.String("lang", "", `Accept-Language to send (default "en-US,en;q=0.9")`)
	showVersion := flag.Bool("version", false, "print the version and exit")
	flag.Usage = func() {
		fmt.Fprintf(flag.CommandLine.Output(), "Usage: cmslens [flags] domain...\n       cmslens -file page.html\n\n")
		flag.PrintDefaults()
	}
	flag.Parse()

	if *showVersion {
		fmt.Println("cmslens", version)
		return
	}
	if *file != "" {
		os.Exit(detectFile(*file, *asJSON))
	}
	if flag.NArg() == 0 {
		flag.Usage()
		os.Exit(2)
	}

	opts := cmslens.Options{ScraperAPIKey: *scraperKey, ScraperCountry: *country, AcceptLanguage: *lang}
	inputs := flag.Args()
	lines := make([]line, len(inputs))
	slots := make(chan struct{}, parallel)
	var wg sync.WaitGroup
	for i, input := range inputs {
		wg.Add(1)
		go func() {
			defer wg.Done()
			slots <- struct{}{}
			defer func() { <-slots }()
			ctx, cancel := context.WithTimeout(context.Background(), *timeout)
			defer cancel()
			res, err := cmslens.Scan(ctx, input, opts)
			lines[i] = line{Input: input, Result: res}
			if err != nil {
				lines[i].Error = err.Error()
			}
		}()
	}
	wg.Wait()

	failed := false
	for _, l := range lines {
		failed = failed || l.Error != ""
		print(l, *asJSON)
	}
	if failed {
		os.Exit(1)
	}
}

func detectFile(path string, asJSON bool) int {
	body, err := os.ReadFile(path)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	detected := cmslens.Detect(string(body), http.Header{})
	print(line{Input: path, Result: cmslens.Result{Status: cmslens.StatusOK, Detected: detected}}, asJSON)
	return 0
}

func print(l line, asJSON bool) {
	if asJSON {
		if l.Result.Detected == nil {
			l.Result.Detected = []string{}
		}
		out, _ := json.Marshal(l)
		fmt.Println(string(out))
		return
	}
	switch {
	case l.Error != "":
		fmt.Printf("%s\terror: %s\n", l.Input, l.Error)
	case l.Result.Status == cmslens.StatusCloudflare:
		fmt.Printf("%s\tblocked by an anti-bot page\n", l.Input)
	case len(l.Result.Detected) == 0:
		fmt.Printf("%s\tunknown\n", l.Input)
	default:
		fmt.Printf("%s\t%s\n", l.Input, strings.Join(l.Result.Detected, ", "))
	}
}
