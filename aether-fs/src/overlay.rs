// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Writable overlay for dynamic package installs.
//
// When a task runs `pip install`, `npm install`, etc., writes go to a
// per-task tmpfs-backed FUSE overlay. At task completion:
//   1. Scan the overlay for useful artifacts (binaries, libraries, configs)
//   2. Extract them to the CAS for future lazy materialization
//   3. Discard the rest (build caches, temp files, etc.)
//
// This keeps the base image clean and allows package installs without
// pre-building every possible dependency combination.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Manages a writable overlay for a single task execution.
pub struct WritableOverlay {
    /// The tmpfs mount point for this overlay
    pub overlay_dir: PathBuf,
    /// Task ID this overlay belongs to
    pub task_id: String,
    /// Paths that were written during execution
    pub written_paths: HashSet<PathBuf>,
}

impl WritableOverlay {
    pub fn new(task_id: &str, base_dir: &Path) -> Self {
        WritableOverlay {
            overlay_dir: base_dir.join("overlays").join(task_id),
            task_id: task_id.to_string(),
            written_paths: HashSet::new(),
        }
    }

    /// Set up the overlay directory (call before task execution).
    pub fn setup(&self) -> Result<()> {
        std::fs::create_dir_all(&self.overlay_dir)
            .context("failed to create overlay directory")?;
        Ok(())
    }

    /// Record a write to the overlay (called by FUSE write handler).
    pub fn record_write(&mut self, path: PathBuf) {
        self.written_paths.insert(path);
    }

    /// After task completion, scan for useful artifacts.
    /// Returns paths that should be extracted to the CAS.
    pub fn extract_useful_artifacts(&self) -> Result<Vec<PathBuf>> {
        let mut useful = Vec::new();

        for path in &self.written_paths {
            if !path.exists() {
                continue;
            }

            // Skip known junk patterns
            if is_disposable(path) {
                continue;
            }

            useful.push(path.clone());
        }

        Ok(useful)
    }

    /// Clean up the overlay after extraction.
    pub fn cleanup(&self) -> Result<()> {
        if self.overlay_dir.exists() {
            std::fs::remove_dir_all(&self.overlay_dir)
                .context("failed to clean up overlay directory")?;
        }
        Ok(())
    }
}

/// Check if a path is disposable (build cache, temp files, etc.)
fn is_disposable(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    // Common disposable patterns
    let disposable_patterns = [
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
    ];

    for pattern in &disposable_patterns {
        if path_str.contains(pattern) {
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
}
