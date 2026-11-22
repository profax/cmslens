// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use std::path::{Path, PathBuf};

const LICENSE_TAG: &str = "SPDX-License-Identifier: MIT";

/// A file copied out of the repository takes its license along: the terms are
/// in the file, not only in LICENSE next to it. The tag sits in the first lines
/// of every source file, after a shebang if there is one.
#[test]
fn every_source_file_names_its_license() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in ["src", "tests"] {
        walk(&root.join(dir), &mut files);
    }
    files.push(root.join("Cargo.toml"));
    assert!(
        files.len() > 5,
        "no source files found: the check checked nothing"
    );
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.lines().take(3).any(|l| l.contains(LICENSE_TAG)),
            "{}: no {LICENSE_TAG:?} in its first lines",
            path.display()
        );
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
