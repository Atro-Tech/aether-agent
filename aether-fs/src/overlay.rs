// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Writable overlay artifact extraction.
//
// At task end, the writable layer contains everything the agent wrote:
// installed packages, temp files, build caches, outputs. This module
// decides what's worth keeping (extract to CAS for future lazy
// materialization) and what to discard.

use std::path::{Path, PathBuf};

/// Scan written paths and return only the useful ones.
/// Junk (build caches, __pycache__, .tmp, etc.) is filtered out.
pub fn extract_useful_artifacts(written_paths: &[PathBuf]) -> Vec<PathBuf> {
    written_paths
        .iter()
        .filter(|p| !is_disposable(p))
        .cloned()
        .collect()
}

/// Check if a path is disposable (build cache, temp files, etc.)
fn is_disposable(path: &Path) -> bool {
    let s = path.to_string_lossy();

    let disposable = [
        "/__pycache__/",
        "/.cache/",
        "/tmp/",
        "/node_modules/.cache/",
        "/.npm/",
        "/.pip/",
        "/build/temp.",
        ".pyc",
        ".pyo",
        ".o",
        ".tmp",
        ".lock",
    ];

    for pattern in &disposable {
        if s.contains(pattern) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_disposable() {
        assert!(is_disposable(Path::new("/usr/lib/python3/__pycache__/foo.pyc")));
        assert!(is_disposable(Path::new("/home/user/.cache/pip/wheels/abc")));
        assert!(!is_disposable(Path::new("/usr/lib/python3/openpyxl/reader.py")));
        assert!(!is_disposable(Path::new("/usr/bin/rclone")));
    }

    #[test]
    fn test_extract_filters_junk() {
        let paths = vec![
            PathBuf::from("/usr/bin/gh"),
            PathBuf::from("/usr/lib/python3/__pycache__/mod.pyc"),
            PathBuf::from("/usr/lib/python3/openpyxl/__init__.py"),
            PathBuf::from("/tmp/build-abc.tmp"),
        ];

        let useful = extract_useful_artifacts(&paths);
        assert_eq!(useful.len(), 2);
        assert!(useful.contains(&PathBuf::from("/usr/bin/gh")));
        assert!(useful.contains(&PathBuf::from("/usr/lib/python3/openpyxl/__init__.py")));
    }
}
