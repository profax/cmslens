package cmslens

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"time"

	"github.com/profax/cmslens/internal/netguard"
)

const (
	primaryFetchTimeout = 8 * time.Second
	bypassFetchTimeout  = 20 * time.Second
	scraperFetchTimeout = 30 * time.Second
	maxScanBytes        = 5 << 20 // a site must not be able to exhaust the scanner's memory with its response
	maxRedirects        = 5
)

const defaultAcceptLanguage = "en-US,en;q=0.9"

// Options tune Scan. The zero value works: no third party is contacted, and
// pages are requested in English.
type Options struct {
	// ScraperAPIKey enables a retry through ScraperAPI (scraperapi.com) when a
	// site blocks the direct request or the scanner's address. Empty means the
	// scan never leaves this machine.
	ScraperAPIKey string
	// ScraperCountry is ScraperAPI's country_code, such as "us" or "de": some
	// sites answer only visitors from their own country. Empty leaves the
	// choice to ScraperAPI.
	ScraperCountry string
	// AcceptLanguage is sent with every request; empty means "en-US,en;q=0.9".
	AcceptLanguage string
}

func scanHeaders(opts Options) http.Header {
	h := http.Header{}
	h.Set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/120.0.0.0")
	h.Set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
	h.Set("Accept-Language", firstNonEmpty(opts.AcceptLanguage, defaultAcceptLanguage))
	return h
}

/*
isWafPage looks for text markers of anti-bot challenge pages. The check is
heuristic (a challenge page does not identify itself with a header), so it is a
set of strings rather than a structural test.
*/
func isWafPage(body string) bool {
	if strings.Contains(body, "rt-solar.ru") ||
		strings.Contains(body, "Подтвердите, что&nbsp;вы человек") ||
		strings.Contains(body, "Подтвердите что вы не робот") ||
		strings.Contains(body, "__cf_chl_opt") ||
		strings.Contains(body, "cf-challenge") ||
		strings.Contains(body, "Checking your browser before accessing") {
		return true
	}
	return strings.Contains(body, "ddos-guard") && len(body) < 20000
}

// Status is the outcome of a scan. StatusFailed is never returned by Scan: the
// caller sets it when neither https nor http could be opened (see Scan).
type Status string

const (
	StatusOK         Status = "ok"
	StatusCloudflare Status = "cloudflare"
	StatusFailed     Status = "failed"
)

// Details describes the response the detection was made from.
type Details struct {
	Title      string `json:"title,omitempty"`
	Server     string `json:"server"`
	StatusCode int    `json:"statusCode"`
}

// Result is what Scan found. Detected holds at most one backend engine and at
// most one presentation layer on top of it, such as [Next.js WordPress].
type Result struct {
	Status    Status   `json:"status"`
	Detected  []string `json:"detected"`
	WafBehind bool     `json:"wafBehind"` // the answer came through the WAF bypass
	Details   Details  `json:"details"`
}

// performInitialFetch tries https and falls back to http. Address selection,
// redirects and the ban on internal networks all live in netguard.Client.
func performInitialFetch(ctx context.Context, client *netguard.Client, domain string) (string, *netguard.Result, error) {
	httpsURL := "https://" + domain
	if res, err := client.Fetch(ctx, httpsURL); err == nil {
		return httpsURL, res, nil
	}
	httpURL := "http://" + domain
	res, err := client.Fetch(ctx, httpURL)
	if err != nil {
		return "", nil, err
	}
	return httpURL, res, nil
}

/*
attemptWafBypass tries ScraperAPI (when a key is set) and a direct request with
certificate checks off in parallel; the first success wins, so a quick failure
of one attempt does not bury the other.
*/
func attemptWafBypass(ctx context.Context, bypassClient, scraperClient *netguard.Client, targetURL string, opts Options) *netguard.Result {
	resultCh := make(chan *netguard.Result, 2)
	pending := 0

	tryFetch := func(client *netguard.Client, u string) {
		res, err := client.Fetch(ctx, u)
		if err != nil || res.StatusCode != 200 || isWafPage(string(res.Body)) {
			resultCh <- nil
			return
		}
		resultCh <- res
	}

	if opts.ScraperAPIKey != "" {
		// No `render=true`: the response is byte for byte the same but costs one
		// credit instead of ten. Signatures read the server's markup, and the direct
		// path does not execute it either; rendering the page would mean checking
		// something other than what the main fetcher sees
		scraperURL := "http://api.scraperapi.com?api_key=" + url.QueryEscape(opts.ScraperAPIKey) +
			"&url=" + url.QueryEscape(targetURL)
		if opts.ScraperCountry != "" {
			scraperURL += "&country_code=" + url.QueryEscape(opts.ScraperCountry)
		}
		pending++
		go tryFetch(scraperClient, scraperURL)
	}

	pending++
	go tryFetch(bypassClient, targetURL)

	for i := 0; i < pending; i++ {
		if res := <-resultCh; res != nil {
			return res
		}
	}
	return nil
}

// ErrNoSuchHost separates a typo in the address from a site we could not reach.
// The difference matters: there is nobody to tell about a name that does not
// exist, while the owner of a live name that does not answer needs to know.
var ErrNoSuchHost = errors.New("no such host")

// ErrInvalidDomain means the input is not a host name Scan will fetch: an IP
// address, a name without a dot, or something that is not a URL at all.
var ErrInvalidDomain = errors.New("not a domain name")

var titleRe = regexp.MustCompile(`(?i)<title>([^<]+)</title>`)

func firstNonEmpty(a, b string) string {
	if a != "" {
		return a
	}
	return b
}

/*
Scan fetches the site and detects what it runs. The input may be a URL or a bare
host; it goes through NormalizeDomain first. Everything network-related goes
through internal/netguard, which checks the address each socket actually opens
to, on every redirect hop and on the WAF bypass too, so a redirect cannot lead
the scanner into an internal network.
*/
func Scan(ctx context.Context, input string, opts Options) (Result, error) {
	domain, ok := NormalizeDomain(input)
	if !ok {
		return Result{}, fmt.Errorf("%w: %q", ErrInvalidDomain, input)
	}
	headers := scanHeaders(opts)
	primaryClient := netguard.New(netguard.Options{Timeout: primaryFetchTimeout, MaxBytes: maxScanBytes, MaxRedirects: maxRedirects, Header: headers})

	targetURL, res, fetchErr := performInitialFetch(ctx, primaryClient, domain)
	if fetchErr != nil {
		// The name is not in DNS: there is nothing to open, and the bypass would
		// answer the same. The check sits here so that a typo in the domain does not
		// cost the user another twenty seconds of waiting
		var dnsErr *net.DNSError
		if errors.As(fetchErr, &dnsErr) {
			return Result{}, fmt.Errorf("%w: %s", ErrNoSuchHost, domain)
		}
		targetURL = "https://" + domain
	}

	statusCode := 0
	respHeaders := http.Header{}
	html := ""
	if fetchErr == nil {
		statusCode = res.StatusCode
		respHeaders = res.Header
		html = string(res.Body)
	}
	// The Server header is taken from the direct response only: the bypass has its
	// own, and ScraperAPI's header must not be passed off as the site's
	serverHeader := strings.ToLower(respHeaders.Get("Server"))

	needsBypass := statusCode == 403 || statusCode == 503 || isWafPage(html)
	if fetchErr != nil {
		// The direct request did not happen at all, but the site may be alive and
		// simply refuse our address: anti-scraping filters block hosting ranges, and
		// some sites do not even answer a SYN from a datacenter. Only a different
		// address helps, so without a key the bypass is not started: a local retry
		// differs from the failed attempt only by the certificate check, and that is
		// not why the connection failed
		needsBypass = opts.ScraperAPIKey != ""
	}

	wafBehind := false
	if needsBypass {
		bypassClient := netguard.New(netguard.Options{Timeout: bypassFetchTimeout, MaxBytes: maxScanBytes, MaxRedirects: maxRedirects, Header: headers, InsecureTLS: true})
		scraperClient := netguard.New(netguard.Options{Timeout: scraperFetchTimeout, MaxBytes: maxScanBytes, MaxRedirects: maxRedirects, Header: headers})

		bypassed := attemptWafBypass(ctx, bypassClient, scraperClient, targetURL, opts)
		if bypassed == nil {
			if fetchErr != nil {
				return Result{}, fetchErr
			}
			return Result{Status: StatusCloudflare, Details: Details{Server: serverHeader, StatusCode: statusCode}}, nil
		}
		statusCode = bypassed.StatusCode
		respHeaders = bypassed.Header
		html = string(bypassed.Body)
		wafBehind = true
	}

	detected := Detect(html, respHeaders)

	title := ""
	if m := titleRe.FindStringSubmatch(html); m != nil {
		title = strings.TrimSpace(m[1])
	}

	return Result{
		Status:    StatusOK,
		Detected:  detected,
		WafBehind: wafBehind,
		Details:   Details{Title: title, Server: firstNonEmpty(serverHeader, "unknown"), StatusCode: statusCode},
	}, nil
}

// Detect names the engine (and the presentation layer on top of it, if any)
// from a page body and its response headers, without any network access. It
// returns nil when nothing scores high enough.
func Detect(body string, header http.Header) []string {
	return SelectWinners(scoreResponse(body, header))
}

// scoreResponse returns scores in table order: SelectWinners breaks ties by it.
// Headers are matched by their expressions directly, since they are short.
func scoreResponse(html string, headers http.Header) []CMSScore {
	hits := signatureIndex.scan(html)
	var out []CMSScore
	for i, entry := range cmsSignatures {
		score := 0.0
		for j, item := range entry.sig.body {
			if signatureIndex.plans[i][j].matches(hits, item.re, html) {
				score += item.weight
			}
		}
		for headerName, items := range entry.sig.headers {
			vals := headers.Values(headerName)
			if len(vals) == 0 {
				continue
			}
			val := strings.Join(vals, ", ")
			for _, item := range items {
				if item.re.MatchString(val) {
					score += item.weight
				}
			}
		}
		if score > 0 {
			out = append(out, CMSScore{Name: entry.name, Score: score})
		}
	}
	return out
}
