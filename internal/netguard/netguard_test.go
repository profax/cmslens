package netguard

import (
	"errors"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"net/url"
	"strings"
	"testing"
)

func TestClosedRangesStayClosed(t *testing.T) {
	closed := []string{
		"127.0.0.1", "127.1.2.3", "0.0.0.0",
		"10.0.0.1", "192.168.1.10", "172.16.0.1", "172.31.255.254",
		"169.254.169.254", // cloud metadata, the reason all of this exists
		"100.64.0.1",      // CGNAT
		"224.0.0.1",       // multicast
		"255.255.255.255", // broadcast
		"192.0.0.1", "192.0.2.5", "198.18.0.1", "198.51.100.7", "203.0.113.9",

		"::", "::1", "fe80::1", "fd00::1", "fc00::1", "ff02::1",
		"::ffff:127.0.0.1", // the same loopback written IPv4-mapped
		"::127.0.0.1",      // and again in the deprecated IPv4-compatible form
		"2002:7f00:1::",    // 6to4: IPv4 embedded in the address
		"64:ff9b::7f00:1",  // NAT64: the same in another way
		"2001::1",          // Teredo
		"fe80::1%eth0",     // a zone must not carry the address past the list
	}
	for _, raw := range closed {
		addr, err := netip.ParseAddr(raw)
		if err != nil {
			t.Fatalf("%s did not parse as an address: %v", raw, err)
		}
		if !IsBlocked(addr) {
			t.Errorf("%s is open but must be closed", raw)
		}
	}
}

func TestPublicAddressesPass(t *testing.T) {
	for _, raw := range []string{"8.8.8.8", "93.184.216.34", "172.32.0.1", "100.128.0.1", "2606:4700::1111", "2a00:1450:4010::200e"} {
		addr, err := netip.ParseAddr(raw)
		if err != nil {
			t.Fatalf("%s did not parse as an address: %v", raw, err)
		}
		if IsBlocked(addr) {
			t.Errorf("%s is closed, but it is an ordinary public address", raw)
		}
	}
}

func TestCheckHostRejectsWhatIsVisibleByName(t *testing.T) {
	for _, host := range []string{"", "localhost", "app.localhost", "LOCALHOST", "wiki", "127.0.0.1", "169.254.169.254", "[::1]"} {
		if err := CheckHost(host); !errors.Is(err, ErrBlocked) {
			t.Errorf("host %q passed, error: %v", host, err)
		}
	}
	for _, host := range []string{"example.com", "www.example.org", "example.com.", "8.8.8.8"} {
		if err := CheckHost(host); err != nil {
			t.Errorf("host %q rejected: %v", host, err)
		}
	}
}

func TestSafeURLRejectsSchemesAndInternalTargets(t *testing.T) {
	for _, raw := range []string{
		"file:///etc/passwd",
		"gopher://example.com",
		"ftp://example.com/pub",
		"not a url",
		"http://127.0.0.1:8090/dashboard",
		"https://192.168.1.1/",
		"http://169.254.169.254/latest/meta-data/",
		"http://[::1]:8088/admin/",
		"http://localhost:5432/",
	} {
		if _, err := SafeURL(raw); !errors.Is(err, ErrBlocked) {
			t.Errorf("%q passed, error: %v", raw, err)
		}
	}
	if _, err := SafeURL("https://example.com/contacts"); err != nil {
		t.Errorf("an ordinary address was rejected: %v", err)
	}
}

func TestDialGuardChecksTheAddressItIsGiven(t *testing.T) {
	for _, address := range []string{"127.0.0.1:80", "169.254.169.254:80", "[::1]:8088", "10.0.0.5:5432", "garbage"} {
		if err := guardDial("tcp4", address, nil); !errors.Is(err, ErrBlocked) {
			t.Errorf("dialing %s allowed, error: %v", address, err)
		}
	}
	if err := guardDial("tcp4", "8.8.8.8:443", nil); err != nil {
		t.Errorf("dialing a public address refused: %v", err)
	}
	// A non-TCP network is either UDP or a unix socket: the fetcher needs neither,
	// and a unix socket leads straight to a database.
	if err := guardDial("unix", "/var/run/postgresql/.s.PGSQL.5432", nil); !errors.Is(err, ErrBlocked) {
		t.Errorf("unix socket allowed, error: %v", err)
	}
}

// The point of the package: a live server on loopback stays unreachable even
// when its name passed. The request bypasses Fetch on purpose, skipping address
// parsing, so only the barrier in the dialer answers here.
func TestLoopbackServerStaysUnreachable(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Write([]byte("secret"))
	}))
	defer server.Close()

	client := New(Options{})
	if _, err := client.http.Get(server.URL); !errors.Is(err, ErrBlocked) {
		t.Fatalf("a connection to loopback opened, error: %v", err)
	}
	if _, err := client.Fetch(t.Context(), server.URL); !errors.Is(err, ErrBlocked) {
		t.Fatalf("Fetch reached loopback, error: %v", err)
	}
}

func TestRedirectPolicy(t *testing.T) {
	client := New(Options{MaxRedirects: 2})

	target, err := url.Parse("file:///etc/passwd")
	if err != nil {
		t.Fatal(err)
	}
	if err := client.http.CheckRedirect(&http.Request{URL: target}, nil); !errors.Is(err, ErrBlocked) {
		t.Errorf("redirect to file:// allowed, error: %v", err)
	}

	allowed, err := url.Parse("https://example.com/next")
	if err != nil {
		t.Fatal(err)
	}
	if err := client.http.CheckRedirect(&http.Request{URL: allowed}, make([]*http.Request, 2)); err != nil {
		t.Errorf("the second redirect was refused: %v", err)
	}
	if err := client.http.CheckRedirect(&http.Request{URL: allowed}, make([]*http.Request, 3)); !errors.Is(err, ErrBlocked) {
		t.Errorf("redirects never end, error: %v", err)
	}
}

func TestReadCappedMarksTruncation(t *testing.T) {
	body, truncated, err := readCapped(strings.NewReader("12345"), 10)
	if err != nil || truncated || string(body) != "12345" {
		t.Errorf("short response read wrong: %q, truncated=%v, %v", body, truncated, err)
	}

	body, truncated, err = readCapped(strings.NewReader("1234567890"), 10)
	if err != nil || truncated || len(body) != 10 {
		t.Errorf("a response exactly at the cap counted as truncated: %d bytes, truncated=%v, %v", len(body), truncated, err)
	}

	body, truncated, err = readCapped(strings.NewReader(strings.Repeat("a", 4096)), 10)
	if err != nil || !truncated || len(body) != 10 {
		t.Errorf("the cap did not work: %d bytes, truncated=%v, %v", len(body), truncated, err)
	}
}
