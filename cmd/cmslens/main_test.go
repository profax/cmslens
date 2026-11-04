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
