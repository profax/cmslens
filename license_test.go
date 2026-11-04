// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

package cmslens

import (
	"bufio"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const licenseTag = "SPDX-License-Identifier: MIT"

// A file copied out of the repository takes its license along: the terms are
// in the file, not only in LICENSE next to it. The tag sits in the first lines
// of every source file, after a shebang if there is one.
func TestEverySourceFileNamesItsLicense(t *testing.T) {
	t.Parallel()

	files := sourceFiles(t)
	if len(files) == 0 {
		t.Fatal("no source files found: the check checked nothing")
	}
	for _, path := range files {
		if !headerHasLicense(t, path) {
			t.Errorf("%s: no %q in its first lines", path, licenseTag)
		}
	}
}

// sourceFiles lists the Go and shell files of the repository, hidden
// directories such as .git left out.
func sourceFiles(t *testing.T) []string {
	t.Helper()
	var files []string
	err := filepath.WalkDir(".", func(path string, entry fs.DirEntry, err error) error {
		switch {
		case err != nil:
			return err
		case entry.IsDir() && path != "." && strings.HasPrefix(entry.Name(), "."):
			return filepath.SkipDir
		case !entry.IsDir() && (filepath.Ext(path) == ".go" || filepath.Ext(path) == ".sh"):
			files = append(files, path)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return files
}

func headerHasLicense(t *testing.T, path string) bool {
	t.Helper()
	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = file.Close() }()

	lines := bufio.NewScanner(file)
	for range 3 {
		if !lines.Scan() {
			break
		}
		if strings.Contains(lines.Text(), licenseTag) {
			return true
		}
	}
	return false
}
