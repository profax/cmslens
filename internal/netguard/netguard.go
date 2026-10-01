// Package netguard is the only way out to the network for code that fetches an
// address supplied by a user.
//
// What is checked is neither the host name nor the result of resolving it, but
// the address the socket is about to connect to. The difference is not
// theoretical: between "resolved and allowed" and "connected" there is room for
// a second DNS answer pointing to 127.0.0.1, and a check by name does not see
// that swap. The ban lives in the dialer's Control hook, where net passes the
// address it has already chosen, so it fires on every attempt: on the host's
// fallback addresses and on every redirect hop, without a single line in the
// calling code.
//
// This also explains what the package does NOT do. It does not resolve the host
// itself and reject it whole when any A record points inside: a host with two
// addresses, one of them internal, stays reachable via the external one, and a
// connection inward simply does not open.
//
// The stakes, stated plainly: http://169.254.169.254/ hands out cloud
// credentials, and services on 127.0.0.1 are a database or an admin panel.
package netguard

import (
	"context"
	"crypto/tls"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strings"
	"syscall"
	"time"
)

// ErrBlocked is the one signal by which a caller tells "not allowed there" from
// "the network did not answer": errors.Is reaches it through the wrappers of
// net and net/http, so the caller needs a single check for all cases.
var ErrBlocked = errors.New("blocked target")

func blockedTarget(what string) error { return fmt.Errorf("%w: %s", ErrBlocked, what) }

// Where the fetcher may not go. The list is explicit rather than "everything
// that is not public": netip's ready-made predicates know private networks and
// loopback, but not 169.254.169.254 (cloud metadata), 100.64.0.0/10 (neighbours
// at the same provider) or 2002::/16 (the same 127.0.0.1 written as IPv6).
var blockedRanges = []netip.Prefix{
	// IPv4
	netip.MustParsePrefix("0.0.0.0/8"),       // "this host", behaves as localhost on Linux
	netip.MustParsePrefix("10.0.0.0/8"),      // private network
	netip.MustParsePrefix("100.64.0.0/10"),   // CGNAT, addresses of neighbours at the same provider
	netip.MustParsePrefix("127.0.0.0/8"),     // all of loopback, not just 127.0.0.1
	netip.MustParsePrefix("169.254.0.0/16"),  // link-local, cloud metadata lives here
	netip.MustParsePrefix("172.16.0.0/12"),   // private network
	netip.MustParsePrefix("192.0.0.0/24"),    // IETF protocol assignments
	netip.MustParsePrefix("192.0.2.0/24"),    // documentation
	netip.MustParsePrefix("192.88.99.0/24"),  // 6to4 relay
	netip.MustParsePrefix("192.168.0.0/16"),  // private network
	netip.MustParsePrefix("198.18.0.0/15"),   // network benchmark testing
	netip.MustParsePrefix("198.51.100.0/24"), // documentation
	netip.MustParsePrefix("203.0.113.0/24"),  // documentation
	netip.MustParsePrefix("224.0.0.0/3"),     // multicast and everything reserved above it, broadcast included

	// IPv6
	netip.MustParsePrefix("::/96"),          // covers ::, ::1 and the deprecated form ::127.0.0.1
	netip.MustParsePrefix("64:ff9b::/96"),   // NAT64, the address carries IPv4 inside
	netip.MustParsePrefix("64:ff9b:1::/48"), // the same, local-use variant
	netip.MustParsePrefix("100::/64"),       // discard prefix
	netip.MustParsePrefix("2001::/23"),      // IETF protocol assignments, Teredo included
	netip.MustParsePrefix("2001:db8::/32"),  // documentation
	netip.MustParsePrefix("2002::/16"),      // 6to4, IPv4 inside the address too
	netip.MustParsePrefix("fc00::/7"),       // unique local addresses, fc and fd
	netip.MustParsePrefix("fe80::/10"),      // link-local
	netip.MustParsePrefix("ff00::/8"),       // multicast
}

// IsBlocked answers a single question: may a connection be opened to this
// address.
func IsBlocked(addr netip.Addr) bool {
	if !addr.IsValid() {
		return true
	}
	// A zone (fe80::1%eth0) makes Prefix.Contains return false for every prefix,
	// so the address would pass the whole list untouched. Unmap turns
	// ::ffff:127.0.0.1 back into IPv4; otherwise it slips past the list the same
	// way.
	addr = addr.WithZone("").Unmap()
	for _, network := range blockedRanges {
		if network.Contains(addr) {
			return true
		}
	}
	return false
}

// CheckHost rejects what is visible from the name alone, spending neither a
// lookup nor a connection. It is a convenience and a clear error at the
// entrance, not the barrier: the barrier is guardDial, and nothing can be
// opened while bypassing it.
func CheckHost(host string) error {
	host = strings.TrimSuffix(strings.Trim(host, "[]"), ".")
	if host == "" {
		return blockedTarget("empty host")
	}
	if addr, err := netip.ParseAddr(host); err == nil {
		if IsBlocked(addr) {
			return blockedTarget(host)
		}
		return nil
	}
	// The resolver completes a name without a dot with the local search suffix and
	// leads it into the machine's network rather than the internet.
	lower := strings.ToLower(host)
	if !strings.Contains(lower, ".") || lower == "localhost" || strings.HasSuffix(lower, ".localhost") {
		return blockedTarget(host)
	}
	return nil
}

// SafeURL parses an address and says whether it may be fetched at all.
func SafeURL(raw string) (*url.URL, error) {
	parsed, err := url.Parse(strings.TrimSpace(raw))
	if err != nil {
		return nil, blockedTarget("unparsable address")
	}
	if err := checkScheme(parsed); err != nil {
		return nil, err
	}
	if err := CheckHost(parsed.Hostname()); err != nil {
		return nil, err
	}
	return parsed, nil
}

func checkScheme(target *url.URL) error {
	if target.Scheme != "http" && target.Scheme != "https" {
		return blockedTarget(fmt.Sprintf("scheme %q", target.Scheme))
	}
	return nil
}

type Options struct {
	Timeout      time.Duration // for the whole request, reading the body included
	MaxBytes     int64
	MaxRedirects int
	Header       http.Header
	// InsecureTLS turns off certificate verification of the target. The SSRF
	// barrier in guardDial does not depend on it: this is a separate property
	// (protection from MITM), not protection of the address. The CMS detector
	// needs it to get past a WAF on sites with an expired or self-signed
	// certificate, so the choice belongs to the caller rather than being a
	// package-wide setting.
	InsecureTLS bool
}

type Client struct {
	http     *http.Client
	timeout  time.Duration
	maxBytes int64
	header   http.Header
}

type Result struct {
	URL        string // after all redirects, not the one we started from
	StatusCode int
	Header     http.Header
	Body       []byte // raw bytes: decoding from legacy charsets is up to the page parser
	Truncated  bool
}

func New(opts Options) *Client {
	timeout := opts.Timeout
	if timeout <= 0 {
		timeout = 5 * time.Second
	}
	maxBytes := opts.MaxBytes
	if maxBytes <= 0 {
		maxBytes = 2 << 20
	}
	maxRedirects := opts.MaxRedirects
	if maxRedirects <= 0 {
		maxRedirects = 3
	}

	dialer := &net.Dialer{Timeout: timeout, KeepAlive: 30 * time.Second, Control: guardDial}
	var tlsConfig *tls.Config
	if opts.InsecureTLS {
		tlsConfig = &tls.Config{InsecureSkipVerify: true}
	}
	return &Client{
		http: &http.Client{
			Transport: &http.Transport{
				// Proxies from the environment are off on purpose: with a proxy the socket
				// opens to the proxy, not to the target, and guardDial would check the
				// proxy's address. The ban on internal networks would cease to exist, and
				// silently.
				Proxy:                 nil,
				DialContext:           dialer.DialContext,
				TLSClientConfig:       tlsConfig,
				ForceAttemptHTTP2:     true,
				MaxIdleConns:          32,
				IdleConnTimeout:       30 * time.Second,
				TLSHandshakeTimeout:   timeout,
				ExpectContinueTimeout: time.Second,
			},
			CheckRedirect: func(req *http.Request, via []*http.Request) error {
				if len(via) > maxRedirects {
					return blockedTarget(fmt.Sprintf("more than %d redirects", maxRedirects))
				}
				// guardDial will check the address of the next hop, but nothing else checks
				// the scheme: Location comes from someone else's server, and a file:// in
				// it is their choice, not ours.
				return checkScheme(req.URL)
			},
		},
		timeout:  timeout,
		maxBytes: maxBytes,
		header:   opts.Header,
	}
}

// Fetch reads the whole page, but no more than the cap: a scanner must not let
// a gigabyte response eat its memory.
func (c *Client) Fetch(ctx context.Context, rawURL string) (*Result, error) {
	target, err := SafeURL(rawURL)
	if err != nil {
		return nil, err
	}

	ctx, cancel := context.WithTimeout(ctx, c.timeout)
	defer cancel()

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, target.String(), nil)
	if err != nil {
		return nil, fmt.Errorf("building request: %w", err)
	}
	for name, values := range c.header {
		req.Header[name] = values
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	body, truncated, err := readCapped(resp.Body, c.maxBytes)
	if err != nil {
		return nil, fmt.Errorf("reading response body: %w", err)
	}
	return &Result{
		URL:        resp.Request.URL.String(),
		StatusCode: resp.StatusCode,
		Header:     resp.Header,
		Body:       body,
		Truncated:  truncated,
	}, nil
}

// readCapped reads one byte more than the cap; otherwise "exactly the cap" and
// "truncated" cannot be told apart.
func readCapped(source io.Reader, maxBytes int64) ([]byte, bool, error) {
	body, err := io.ReadAll(io.LimitReader(source, maxBytes+1))
	if err != nil {
		return nil, false, err
	}
	if int64(len(body)) > maxBytes {
		return body[:maxBytes], true, nil
	}
	return body, false, nil
}

// guardDial is the barrier. net calls it after resolving and before connect,
// passing the very address the socket is about to open to.
func guardDial(network, address string, _ syscall.RawConn) error {
	if !strings.HasPrefix(network, "tcp") {
		return blockedTarget(fmt.Sprintf("network %q", network))
	}
	host, _, err := net.SplitHostPort(address)
	if err != nil {
		return blockedTarget(address)
	}
	addr, err := netip.ParseAddr(host)
	if err != nil {
		return blockedTarget(address)
	}
	if IsBlocked(addr) {
		return blockedTarget(addr.String())
	}
	return nil
}
