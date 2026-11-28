// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

package cmslens

import (
	"os"
	"regexp"
	"strings"
	"testing"
)

// A release published a moment ago may be broken or malicious: Dependabot
// waits a week before it proposes one, in every ecosystem it watches.
func TestEveryDependabotEcosystemHasCooldown(t *testing.T) {
	t.Parallel()

	data, err := os.ReadFile(".github/dependabot.yml")
	if err != nil {
		t.Fatal(err)
	}
	entries := strings.Split(string(data), "- package-ecosystem:")[1:]
	if len(entries) == 0 {
		t.Fatal("no package ecosystems found: the check checked nothing")
	}
	cooldown := regexp.MustCompile(`(?m)^    cooldown:\n      default-days: 7$`)
	for _, entry := range entries {
		if !cooldown.MatchString(entry) {
			name, _, _ := strings.Cut(entry, "\n")
			t.Errorf("%s: no cooldown with default-days: 7", strings.TrimSpace(name))
		}
	}
}
