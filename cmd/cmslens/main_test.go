// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

package main

import (
	"os"
	"os/exec"
	"strings"
	"testing"
)

// TestMain turns the test binary into cmslens itself when this variable is
// set, so a test can run the command the way a user does.
const runMainEnv = "CMSLENS_TEST_RUN_MAIN"

func TestMain(m *testing.M) {
	if os.Getenv(runMainEnv) != "" {
		main()
		os.Exit(0)
	}
	os.Exit(m.Run())
}

// The help text printed the ScraperAPI key from the environment as the flag's
// default value: a terminal, a log or a screenshot of cmslens -h gave the key
// away.
func TestHelpDoesNotShowTheKey(t *testing.T) {
	t.Parallel()

	const key = "test-key-not-for-real-use"
	cmd := exec.CommandContext(t.Context(), os.Args[0], "-h") //nolint:gosec // the test binary runs itself as cmslens
	cmd.Env = append(os.Environ(), runMainEnv+"=1", "SCRAPERAPI_KEY="+key)
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("cmslens -h: %v\n%s", err, out)
	}
	if strings.Contains(string(out), key) {
		t.Errorf("the help text shows the key:\n%s", out)
	}
	if !strings.Contains(string(out), "-scraperapi-key") {
		t.Errorf("the help text lost the flag:\n%s", out)
	}
}

// go install printed "cmslens v0.2.0" and the release archive and the Rust
// build "cmslens 0.2.0", so the line a bug report quotes depended on how the
// command was installed.
func TestVersionHasOneForm(t *testing.T) {
	t.Parallel()

	cases := []struct{ name, set, module, want string }{
		{"release archive", "0.2.0", "v0.2.0", "0.2.0"},
		{"go install", "dev", "v0.2.0", "0.2.0"},
		{"go build from a checkout", "dev", "v0.2.1-0.20261126120000-0123456789ab", "0.2.1-0.20261126120000-0123456789ab"},
		{"no module version", "dev", "(devel)", "dev"},
		{"no build info", "dev", "", "dev"},
	}
	for _, c := range cases {
		if got := printedVersion(c.set, c.module); got != c.want {
			t.Errorf("%s: printedVersion(%q, %q) = %q, want %q", c.name, c.set, c.module, got, c.want)
		}
	}
}
