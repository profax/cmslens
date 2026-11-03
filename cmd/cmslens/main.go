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
	"io"
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

// moduleVersion is the version to print: the one the release build set, or the
// module version go install recorded.
func moduleVersion() string {
	info, ok := debug.ReadBuildInfo()
	if ok && version == "dev" && info.Main.Version != "" && info.Main.Version != "(devel)" {
		return info.Main.Version
	}
	return version
}

const (
	// Sites are fetched in parallel, but not so many at once that a long list
	// looks like an attack from this machine.
	parallel       = 8
	defaultTimeout = 60 * time.Second
	exitUsage      = 2
)

const usage = "Usage: cmslens [flags] domain...\n       cmslens -file page.html\n\n"

type line struct {
	Input  string         `json:"input"`
	Result cmslens.Result `json:"result"`
	Error  string         `json:"error,omitempty"`
}

func main() {
	asJSON := flag.Bool("json", false, "print one JSON object per site")
	file := flag.String("file", "", "detect from a saved HTML page instead of fetching (no headers are used)")
	timeout := flag.Duration("timeout", defaultTimeout, "time limit per site")
	scraperKey := flag.String("scraperapi-key", os.Getenv("SCRAPERAPI_KEY"),
		"retry blocked sites through ScraperAPI (default $SCRAPERAPI_KEY)")
	country := flag.String("country", "", "ScraperAPI country code, such as us or de")
	lang := flag.String("lang", "", `Accept-Language to send (default "en-US,en;q=0.9")`)
	showVersion := flag.Bool("version", false, "print the version and exit")
	flag.Usage = func() {
		// There is nowhere left to report a failed write of the usage text
		_, _ = io.WriteString(flag.CommandLine.Output(), usage)
		flag.PrintDefaults()
	}
	flag.Parse()

	if *showVersion {
		fmt.Println("cmslens", moduleVersion())
		return
	}
	if *file != "" {
		os.Exit(detectFile(*file, *asJSON))
	}
	if flag.NArg() == 0 {
		flag.Usage()
		os.Exit(exitUsage)
	}

	opts := cmslens.Options{ScraperAPIKey: *scraperKey, ScraperCountry: *country, AcceptLanguage: *lang}
	failed := false
	for _, l := range scanAll(flag.Args(), opts, *timeout) {
		failed = failed || l.Error != ""
		if err := printLine(l, *asJSON); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}
	if failed {
		os.Exit(1)
	}
}

// scanAll scans the sites in parallel and returns the lines in input order.
func scanAll(inputs []string, opts cmslens.Options, timeout time.Duration) []line {
	lines := make([]line, len(inputs))
	slots := make(chan struct{}, parallel)
	var wg sync.WaitGroup
	for i, input := range inputs {
		wg.Add(1)
		go func() {
			defer wg.Done()
			slots <- struct{}{}
			defer func() { <-slots }()
			ctx, cancel := context.WithTimeout(context.Background(), timeout)
			defer cancel()
			res, err := cmslens.Scan(ctx, input, opts)
			lines[i] = line{Input: input, Result: res}
			if err != nil {
				lines[i].Error = err.Error()
			}
		}()
	}
	wg.Wait()
	return lines
}

func detectFile(path string, asJSON bool) int {
	body, err := os.ReadFile(path) //nolint:gosec // reading the file the user named is the point of -file
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	detected := cmslens.Detect(string(body), http.Header{})
	err = printLine(line{Input: path, Result: cmslens.Result{Status: cmslens.StatusOK, Detected: detected}}, asJSON)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	return 0
}

func printLine(l line, asJSON bool) error {
	if asJSON {
		if l.Result.Detected == nil {
			l.Result.Detected = []string{}
		}
		out, err := json.Marshal(l)
		if err != nil {
			return fmt.Errorf("encoding the result of %s: %w", l.Input, err)
		}
		fmt.Println(string(out))
		return nil
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
	return nil
}
