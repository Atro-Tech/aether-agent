// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// AgentFS unified tree: stacks all layers into a single filesystem view.
//
// This is the core of AgentFS. It resolves paths across layers:
//   1. Writable layer (top) — all writes land here
//   2. Add-in layers (middle) — newest first, files appear on register
//   3. Base template (bottom) — read-only host directory or squashfs
//
// The agent sees one normal Linux filesystem. It never knows about layers.

use std::collections::HashSet;
use std::io;
use std::sync::{Arc, RwLock};

use crate::layer::{AddInLayer, BaseLayer, FileMeta, Layer, WritableLayer};

/// The unified filesystem tree.
/// Resolves reads top-down, writes always go to the writable layer.
pub struct FsTree {
    /// Top: captures all writes (tmpfs-backed)
    writable: WritableLayer,

    /// Middle: add-in packages (newest first).
    /// Protected by RwLock so add-ins can be registered while the
    /// filesystem is mounted and serving reads.
    addins: Arc<RwLock<Vec<AddInLayer>>>,

    /// Bottom: the base template (read-only)
    base: BaseLayer,
}

impl FsTree {
    pub fn new(base: BaseLayer, writable: WritableLayer) -> Self {
        FsTree {
            writable,
            addins: Arc::new(RwLock::new(Vec::new())),
            base,
        }
    }

    /// Get a handle to the add-in list for registering new layers.
    pub fn addin_handle(&self) -> Arc<RwLock<Vec<AddInLayer>>> {
        self.addins.clone()
    }

    // ─── Read operations (resolve top-down) ───

    /// Check if a path exists in any layer.
    pub fn exists(&self, path: &str) -> bool {
        if self.writable.exists(path) {
            return true;
        }

        if let Ok(addins) = self.addins.read() {
            for layer in addins.iter() {
                if layer.exists(path) {
                    return true;
                }
            }
        }

        self.base.exists(path)
    }

    /// Get metadata for a path. First layer that has it wins.
    pub fn metadata(&self, path: &str) -> Option<FileMeta> {
        if let Some(meta) = self.writable.metadata(path) {
            return Some(meta);
        }

        if let Ok(addins) = self.addins.read() {
            for layer in addins.iter() {
                if let Some(meta) = layer.metadata(path) {
                    return Some(meta);
                }
            }
        }

        self.base.metadata(path)
    }

    /// Read file content. First layer that has it wins.
    pub fn read(&self, path: &str) -> Option<io::Result<Vec<u8>>> {
        // Writable layer first (user may have overwritten a base file)
        if let Some(result) = self.writable.read(path) {
            return Some(result);
        }

        // Add-in layers (newest first)
        if let Ok(addins) = self.addins.read() {
            for layer in addins.iter() {
                if let Some(result) = layer.read(path) {
                    return Some(result);
                }
            }
        }

        // Base template
        self.base.read(path)
    }

    /// List directory entries. Merges across all layers, deduplicates.
    pub fn readdir(&self, path: &str) -> Option<Vec<String>> {
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        let mut found_any = false;

        // Writable layer
        if let Some(names) = self.writable.readdir(path) {
            found_any = true;
            for name in names {
                if seen.insert(name.clone()) {
                    entries.push(name);
                }
            }
        }

        // Add-in layers
        if let Ok(addins) = self.addins.read() {
            for layer in addins.iter() {
                if let Some(names) = layer.readdir(path) {
                    found_any = true;
                    for name in names {
                        if seen.insert(name.clone()) {
                            entries.push(name);
                        }
                    }
                }
            }
        }

        // Base template
        if let Some(names) = self.base.readdir(path) {
            found_any = true;
            for name in names {
                if seen.insert(name.clone()) {
                    entries.push(name);
                }
            }
        }

        if found_any {
            entries.sort();
            Some(entries)
        } else {
            None
        }
    }

    // ─── Write operations (always to writable layer) ───

    /// Write a file. Always goes to the writable layer.
    pub fn write(&self, path: &str, data: &[u8]) -> io::Result<()> {
        self.writable.write_file(path, data)
    }

    /// Create a directory. Always in the writable layer.
    pub fn mkdir(&self, path: &str) -> io::Result<()> {
        self.writable.mkdir(path)
    }

    /// Remove a file or directory.
    /// Only removes from the writable layer. If the file exists in a
    /// lower layer, it will "reappear" — a proper whiteout mechanism
    /// would be needed to fully hide it. For v1, this is sufficient
    /// since agents rarely delete base files.
    pub fn remove(&self, path: &str) -> io::Result<()> {
        self.writable.remove(path)
    }

    // ─── Artifact extraction ───

    /// Get all paths written during this task (for extraction at end).
    pub fn written_paths(&self) -> Vec<std::path::PathBuf> {
        self.writable.written_paths()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_tree() -> (tempfile::TempDir, tempfile::TempDir, FsTree) {
        let base_dir = tempfile::tempdir().unwrap();
        let writable_dir = tempfile::tempdir().unwrap();

        // Create base template files
        fs::create_dir_all(base_dir.path().join("usr/bin")).unwrap();
        fs::create_dir_all(base_dir.path().join("etc")).unwrap();
        fs::write(base_dir.path().join("usr/bin/ls"), b"real-ls").unwrap();
        fs::write(base_dir.path().join("etc/hostname"), b"aether").unwrap();

        let base = BaseLayer::new(base_dir.path());
        let writable = WritableLayer::new(writable_dir.path());
        let tree = FsTree::new(base, writable);

        (base_dir, writable_dir, tree)
    }

    #[test]
    fn test_reads_base_files() {
        let (_bd, _wd, tree) = setup_tree();
        assert!(tree.exists("/usr/bin/ls"));
        let data = tree.read("/usr/bin/ls").unwrap().unwrap();
        assert_eq!(data, b"real-ls");
    }

    #[test]
    fn test_addin_files_appear_at_normal_paths() {
        let (_bd, _wd, tree) = setup_tree();

        // Register an add-in
        let mut gh_layer = AddInLayer::new("gh-abc123");
        gh_layer.put_file("/usr/bin/gh", b"gh-binary-content".to_vec(), 0o755);

        tree.addin_handle().write().unwrap().push(gh_layer);

        // /usr/bin/gh now exists alongside /usr/bin/ls
        assert!(tree.exists("/usr/bin/gh"));
        assert!(tree.exists("/usr/bin/ls"));

        // readdir merges entries from both layers
        let entries = tree.readdir("/usr/bin").unwrap();
        assert!(entries.contains(&"ls".to_string()));
        assert!(entries.contains(&"gh".to_string()));
    }

    #[test]
    fn test_writes_go_to_writable_layer() {
        let (_bd, _wd, tree) = setup_tree();

        tree.write("/tmp/output.txt", b"hello").unwrap();
        assert!(tree.exists("/tmp/output.txt"));

        let data = tree.read("/tmp/output.txt").unwrap().unwrap();
        assert_eq!(data, b"hello");
    }

    #[test]
    fn test_writable_overrides_base() {
        let (_bd, _wd, tree) = setup_tree();

        // Base has /etc/hostname = "aether"
        let data = tree.read("/etc/hostname").unwrap().unwrap();
        assert_eq!(data, b"aether");

        // Write overwrites it in the writable layer
        tree.write("/etc/hostname", b"custom-host").unwrap();

        // Now reads return the writable version
        let data = tree.read("/etc/hostname").unwrap().unwrap();
        assert_eq!(data, b"custom-host");
    }

    #[test]
    fn test_readdir_merges_all_layers() {
        let (_bd, _wd, tree) = setup_tree();

        // Add-in provides /usr/bin/gh
        let mut gh = AddInLayer::new("gh");
        gh.put_file("/usr/bin/gh", b"gh-binary".to_vec(), 0o755);
        tree.addin_handle().write().unwrap().push(gh);

        // Writable layer has /usr/bin/custom-tool
        tree.write("/usr/bin/custom-tool", b"#!/bin/sh").unwrap();

        // readdir shows all three: ls (base) + gh (addin) + custom-tool (writable)
        let entries = tree.readdir("/usr/bin").unwrap();
        assert!(entries.contains(&"ls".to_string()));
        assert!(entries.contains(&"gh".to_string()));
        assert!(entries.contains(&"custom-tool".to_string()));
    }
}
