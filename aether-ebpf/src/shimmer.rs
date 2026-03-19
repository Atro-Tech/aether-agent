// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Shimmer: real-time file I/O monitoring using eBPF (Falco/Tetragon style).
//
// Shimmer watches file operations (open, read, write, exec) and applies
// static-pattern scoring per effect. Each file access gets a confidence score:
//   - 0-30: allow (normal package behavior)
//   - 31-70: alert (log the access, allow it)
//   - 71-100: block (deny the access)
//
// Scores come from per-package effect manifests. The eBPF probe reads from
// the shimmer_scores pinned map and makes inline allow/alert/block decisions.

use anyhow::{Context, Result};
use std::path::Path;

use crate::maps;

/// A Shimmer rule from a package manifest.
/// Maps a file path pattern to a confidence score and action.
#[derive(Debug, Clone)]
pub struct ShimmerRule {
    pub path_pattern: String,
    pub score: u32,
    pub action: ShimmerAction,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShimmerAction {
    Allow = 0,
    Alert = 1,
    Block = 2,
}

impl ShimmerAction {
    pub fn from_score(score: u32) -> Self {
        match score {
            0..=30 => ShimmerAction::Allow,
            31..=70 => ShimmerAction::Alert,
            _ => ShimmerAction::Block,
        }
    }
}

/// Shimmer engine: manages file I/O scoring rules.
pub struct Shimmer {
    rules: Vec<ShimmerRule>,
}

impl Shimmer {
    pub fn new() -> Self {
        Shimmer { rules: Vec::new() }
    }

    /// Add rules from a package's effect definition.
    pub fn add_rules(&mut self, rules: Vec<ShimmerRule>) {
        self.rules.extend(rules);
    }

    /// Evaluate a file path against all rules.
    /// Returns the highest-scoring match (most restrictive wins).
    pub fn evaluate(&self, path: &str) -> (u32, ShimmerAction) {
        let mut max_score = 0u32;

        for rule in &self.rules {
            if !path_matches(path, &rule.path_pattern) {
                continue;
            }
            if rule.score > max_score {
                max_score = rule.score;
            }
        }

        (max_score, ShimmerAction::from_score(max_score))
    }

    /// Flush all rules to the eBPF pinned map for kernel-space enforcement.
    pub fn flush_to_ebpf(&self) -> Result<()> {
        let staging_dir = Path::new("/run/aether/ebpf-staging/shimmer");
        std::fs::create_dir_all(staging_dir)
            .context("failed to create shimmer staging directory")?;

        for rule in &self.rules {
            let entry = maps::ShimmerEntry {
                score: rule.score,
                action: rule.action as u32,
            };

            // Write pattern -> entry mapping for the BPF loader
            let filename = rule.path_pattern.replace('/', "_");
            let data = format!("{}|{}|{}", rule.path_pattern, entry.score, entry.action);
            std::fs::write(staging_dir.join(&filename), data)?;
        }

        Ok(())
    }
}

/// Simple glob-style path matching.
fn path_matches(path: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.ends_with("/*") {
        let prefix = &pattern[..pattern.len() - 2];
        return path.starts_with(prefix);
    }
    if pattern.starts_with("*.") {
        let suffix = &pattern[1..]; // e.g. ".so"
        return path.ends_with(suffix);
    }
    path == pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_matches() {
        assert!(path_matches("/usr/lib/foo.so", "*.so"));
        assert!(path_matches("/tmp/data/file.txt", "/tmp/*"));
        assert!(!path_matches("/etc/passwd", "/tmp/*"));
        assert!(path_matches("/anything", "*"));
    }

    #[test]
    fn test_evaluate_highest_score_wins() {
        let mut s = Shimmer::new();
        s.add_rules(vec![
            ShimmerRule {
                path_pattern: "/tmp/*".to_string(),
                score: 10,
                action: ShimmerAction::Allow,
            },
            ShimmerRule {
                path_pattern: "/tmp/*".to_string(),
                score: 50,
                action: ShimmerAction::Alert,
            },
        ]);

        let (score, action) = s.evaluate("/tmp/foo");
        assert_eq!(score, 50);
        assert_eq!(action, ShimmerAction::Alert);
    }
}
