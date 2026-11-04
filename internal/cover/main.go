// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

// Command cover measures test coverage the way CI checks it: the number of
// statements without a test, per file. It writes coverage.txt and the coverage
// badge in both READMEs; CI runs it and fails when the result differs from
// what is committed.
//
//	go run ./internal/cover
//
// The measure is a count, not a percentage: the percentage falls when covered
// code is deleted and rises when a large change comes with a couple of tests,
// while the count moves only when a statement is left without a test or gets
// one. The command's own package is left out of the measure.
package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"maps"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"slices"
	"strconv"
	"strings"
)

const (
	reportFile = "coverage.txt"
	selfPath   = "/internal/cover"
	// A committed file of the repository, read by everyone.
	reportMode = 0o644
)

const reportHeader = `# Statements without a test, per file. Written by go run ./internal/cover;
# CI fails when this file differs from what the tests give.
`

var badgeRe = regexp.MustCompile(`https://img\.shields\.io/badge/coverage-[0-9.]+%25-[a-z]+`)

var (
	errProfileLine = errors.New("unexpected profile line")
	errNothing     = errors.New("the profile has no blocks: nothing was measured")
	errNoBadge     = errors.New("no coverage badge")
)

// Coverage tiers of the badge color, in per mille.
const (
	tierHigh   = 900
	tierMiddle = 750
	tierLow    = 600
)

type count struct {
	total, uncovered int
}

func main() {
	if err := run(context.Background()); err != nil {
		fmt.Fprintln(os.Stderr, "cover:", err)
		os.Exit(1)
	}
}

func run(ctx context.Context) error {
	module, err := goOutput(ctx, "list", "-m")
	if err != nil {
		return err
	}
	list, err := goOutput(ctx, "list", "./...")
	if err != nil {
		return err
	}
	pkgs := slices.DeleteFunc(strings.Fields(list), func(p string) bool { return p == module+selfPath })

	dir, err := os.MkdirTemp("", "cmslens-cover")
	if err != nil {
		return fmt.Errorf("temporary directory: %w", err)
	}
	defer func() { _ = os.RemoveAll(dir) }()
	profile := filepath.Join(dir, "cover.out")

	args := append([]string{"test", "-count=1", "-covermode=set", "-coverprofile=" + profile}, pkgs...)
	test := exec.CommandContext(ctx, "go", args...) //nolint:gosec // the go tool with package names from go list
	test.Stdout = os.Stderr
	test.Stderr = os.Stderr
	if err = test.Run(); err != nil {
		return fmt.Errorf("go test: %w", err)
	}

	raw, err := os.ReadFile(profile) //nolint:gosec // the profile go test has just written
	if err != nil {
		return fmt.Errorf("reading the profile: %w", err)
	}
	counts, err := parseProfile(raw, module+"/")
	if err != nil {
		return err
	}
	report, permille := render(counts)
	if err = os.WriteFile(reportFile, report, reportMode); err != nil {
		return fmt.Errorf("writing %s: %w", reportFile, err)
	}
	for _, name := range []string{"README.md", "README.ru.md"} {
		if err = updateBadge(name, permille); err != nil {
			return err
		}
	}
	fmt.Fprintf(os.Stderr, "cover: %s covered, see %s\n", percent(permille), reportFile)
	return nil
}

func goOutput(ctx context.Context, args ...string) (string, error) {
	out, err := exec.CommandContext(ctx, "go", args...).Output() //nolint:gosec // the go tool with fixed arguments
	if err != nil {
		return "", fmt.Errorf("go %s: %w", strings.Join(args, " "), err)
	}
	return strings.TrimSpace(string(out)), nil
}

// parseProfile sums statements per file. A block listed twice counts once, as
// covered when any of its copies ran.
func parseProfile(raw []byte, prefix string) (map[string]count, error) {
	blocks := map[string]bool{}
	sizes := map[string]int{}
	for line := range strings.Lines(string(raw)) {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "mode:") {
			continue
		}
		fields := strings.Fields(line)
		if len(fields) != 3 { //nolint:mnd // profile lines hold block, statements and hits
			return nil, fmt.Errorf("%w: %q", errProfileLine, line)
		}
		stmts, err := strconv.Atoi(fields[1])
		if err != nil {
			return nil, fmt.Errorf("profile line %q: %w", line, err)
		}
		block := fields[0]
		sizes[block] = stmts
		blocks[block] = blocks[block] || fields[2] != "0"
	}
	if len(blocks) == 0 {
		return nil, errNothing
	}

	counts := map[string]count{}
	for block, covered := range blocks {
		file, _, _ := strings.Cut(strings.TrimPrefix(block, prefix), ":")
		c := counts[file]
		c.total += sizes[block]
		if !covered {
			c.uncovered += sizes[block]
		}
		counts[file] = c
	}
	return counts, nil
}

// render lays out the report and returns the covered share in per mille,
// rounded down so that a single statement without a test never shows as 100%.
func render(counts map[string]count) ([]byte, int) {
	var buf bytes.Buffer
	buf.WriteString(reportHeader)
	var all count
	for _, file := range slices.Sorted(maps.Keys(counts)) {
		c := counts[file]
		fmt.Fprintf(&buf, "%s %d of %d\n", file, c.uncovered, c.total)
		all.total += c.total
		all.uncovered += c.uncovered
	}
	permille := (all.total - all.uncovered) * 1000 / all.total //nolint:mnd // per mille
	fmt.Fprintf(&buf, "total %d of %d, %s covered\n", all.uncovered, all.total, percent(permille))
	return buf.Bytes(), permille
}

func percent(permille int) string {
	return fmt.Sprintf("%d.%d%%", permille/10, permille%10) //nolint:mnd // per mille to percent
}

func badgeColor(permille int) string {
	switch {
	case permille >= tierHigh:
		return "brightgreen"
	case permille >= tierMiddle:
		return "green"
	case permille >= tierLow:
		return "yellow"
	default:
		return "red"
	}
}

// updateBadge rewrites the coverage badge; a README without one is an error, so
// that the badge cannot silently stop following the measure.
func updateBadge(name string, permille int) error {
	text, err := os.ReadFile(name) //nolint:gosec // one of the two READMEs of this repository
	if err != nil {
		return fmt.Errorf("reading %s: %w", name, err)
	}
	if !badgeRe.Match(text) {
		return fmt.Errorf("%w in %s", errNoBadge, name)
	}
	badge := fmt.Sprintf("https://img.shields.io/badge/coverage-%s-%s",
		strings.ReplaceAll(percent(permille), "%", "%25"), badgeColor(permille))
	updated := badgeRe.ReplaceAll(text, []byte(badge))
	err = os.WriteFile(name, updated, reportMode) //nolint:gosec // one of the two READMEs of this repository
	if err != nil {
		return fmt.Errorf("writing %s: %w", name, err)
	}
	return nil
}
