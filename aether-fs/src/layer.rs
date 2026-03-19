// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Filesystem layers. AgentFS stacks three layer types into a unified tree.
//
// Resolution order: Writable → AddIn (newest first) → Base
// First layer that has the path wins.
//
// IMPORTANT: The agent never fetches files. The control plane pushes
// content into layers from the outside. Files just appear.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ─── File metadata ───

#[derive(Debug, Clone)]
pub struct FileMeta {
    pub size: u64,
    pub mode: u32,
    pub is_dir: bool,
    pub modified: SystemTime,
}

impl FileMeta {
    pub fn dir(mode: u32) -> Self {
        FileMeta {
            size: 0,
            mode,
            is_dir: true,
            modified: SystemTime::now(),
        }
    }

    pub fn file(size: u64, mode: u32) -> Self {
        FileMeta {
            size,
            mode,
            is_dir: false,
            modified: SystemTime::now(),
        }
    }
}

// ─── Layer trait ───

pub trait Layer: Send + Sync {
    fn exists(&self, path: &str) -> bool;
    fn metadata(&self, path: &str) -> Option<FileMeta>;
    fn read(&self, path: &str) -> Option<io::Result<Vec<u8>>>;
    fn readdir(&self, path: &str) -> Option<Vec<String>>;
    fn name(&self) -> &str;
}

// ─── BaseLayer: read-only template directory ───

pub struct BaseLayer {
    root: PathBuf,
}

impl BaseLayer {
    pub fn new(root: &Path) -> Self {
        BaseLayer {
            root: root.to_path_buf(),
        }
    }

    fn host_path(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }
}

impl Layer for BaseLayer {
    fn exists(&self, path: &str) -> bool {
        self.host_path(path).exists()
    }

    fn metadata(&self, path: &str) -> Option<FileMeta> {
        let meta = std::fs::metadata(self.host_path(path)).ok()?;
        Some(FileMeta {
            size: meta.len(),
            mode: if meta.is_dir() { 0o755 } else { 0o644 },
            is_dir: meta.is_dir(),
            modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        })
    }

    fn read(&self, path: &str) -> Option<io::Result<Vec<u8>>> {
        let host = self.host_path(path);
        if !host.is_file() {
            return None;
        }
        Some(std::fs::read(&host))
    }

    fn readdir(&self, path: &str) -> Option<Vec<String>> {
        let entries = std::fs::read_dir(self.host_path(path)).ok()?;
        let names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        Some(names)
    }

    fn name(&self) -> &str {
        "base"
    }
}

// ─── AddInLayer: files pushed in by the control plane ───
//
// The control plane reads files from object storage (S3/R2/GCS),
// then pushes the bytes into this layer. The agent never fetches.
// Files appear because the control plane put them here.

pub struct AddInLayer {
    pub addin_id: String,

    /// path → file content (the actual bytes, pushed by control plane)
    files: HashMap<String, AddInFile>,

    /// directories created by the files (derived from paths)
    dirs: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
struct AddInFile {
    content: Vec<u8>,
    meta: FileMeta,
}

impl AddInLayer {
    pub fn new(addin_id: &str) -> Self {
        AddInLayer {
            addin_id: addin_id.to_string(),
            files: HashMap::new(),
            dirs: HashMap::new(),
        }
    }

    /// Put a file into this layer. Called by the control plane.
    /// The bytes are already here — no fetching, no CAS, no lazy loading.
    pub fn put_file(&mut self, path: &str, content: Vec<u8>, mode: u32) {
        let size = content.len() as u64;
        self.files.insert(
            path.to_string(),
            AddInFile {
                content,
                meta: FileMeta::file(size, mode),
            },
        );
        self.register_parent_dirs(path);
    }

    fn register_parent_dirs(&mut self, file_path: &str) {
        let path = Path::new(file_path);

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let mut current = path.parent();
        let mut child_name = filename;

        while let Some(dir) = current {
            let dir_str = dir.to_string_lossy().to_string();
            if dir_str.is_empty() {
                break;
            }

            let entries = self.dirs.entry(dir_str.clone()).or_default();
            if !entries.contains(&child_name) {
                entries.push(child_name.clone());
            }

            if dir_str == "/" {
                break;
            }

            child_name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            current = dir.parent();
        }
    }
}

impl Layer for AddInLayer {
    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path) || self.dirs.contains_key(path)
    }

    fn metadata(&self, path: &str) -> Option<FileMeta> {
        if let Some(file) = self.files.get(path) {
            return Some(file.meta.clone());
        }
        if self.dirs.contains_key(path) {
            return Some(FileMeta::dir(0o755));
        }
        None
    }

    fn read(&self, path: &str) -> Option<io::Result<Vec<u8>>> {
        let file = self.files.get(path)?;
        Some(Ok(file.content.clone()))
    }

    fn readdir(&self, path: &str) -> Option<Vec<String>> {
        self.dirs.get(path).cloned()
    }

    fn name(&self) -> &str {
        &self.addin_id
    }
}

// ─── WritableLayer: captures all writes from inside ───

pub struct WritableLayer {
    root: PathBuf,
}

impl WritableLayer {
    pub fn new(root: &Path) -> Self {
        std::fs::create_dir_all(root).ok();
        WritableLayer {
            root: root.to_path_buf(),
        }
    }

    fn host_path(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }

    pub fn write_file(&self, path: &str, data: &[u8]) -> io::Result<()> {
        let host = self.host_path(path);
        if let Some(parent) = host.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&host, data)
    }

    pub fn mkdir(&self, path: &str) -> io::Result<()> {
        std::fs::create_dir_all(self.host_path(path))
    }

    pub fn remove(&self, path: &str) -> io::Result<()> {
        let host = self.host_path(path);
        if host.is_dir() {
            std::fs::remove_dir_all(&host)
        } else {
            std::fs::remove_file(&host)
        }
    }

    pub fn written_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        collect_files_recursive(&self.root, &self.root, &mut paths);
        paths
    }
}

impl Layer for WritableLayer {
    fn exists(&self, path: &str) -> bool {
        self.host_path(path).exists()
    }

    fn metadata(&self, path: &str) -> Option<FileMeta> {
        let meta = std::fs::metadata(self.host_path(path)).ok()?;
        Some(FileMeta {
            size: meta.len(),
            mode: if meta.is_dir() { 0o755 } else { 0o644 },
            is_dir: meta.is_dir(),
            modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        })
    }

    fn read(&self, path: &str) -> Option<io::Result<Vec<u8>>> {
        let host = self.host_path(path);
        if !host.is_file() {
            return None;
        }
        Some(std::fs::read(&host))
    }

    fn readdir(&self, path: &str) -> Option<Vec<String>> {
        let entries = std::fs::read_dir(self.host_path(path)).ok()?;
        let names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        if names.is_empty() { None } else { Some(names) }
    }

    fn name(&self) -> &str {
        "writable"
    }
}

fn collect_files_recursive(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, root, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(PathBuf::from("/").join(rel));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_base_layer() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("usr/bin")).unwrap();
        fs::write(dir.path().join("usr/bin/ls"), b"fake-ls").unwrap();

        let base = BaseLayer::new(dir.path());
        assert!(base.exists("/usr/bin/ls"));
        assert!(!base.exists("/usr/bin/gh"));

        let data = base.read("/usr/bin/ls").unwrap().unwrap();
        assert_eq!(data, b"fake-ls");
    }

    #[test]
    fn test_addin_layer_files_pushed_by_control_plane() {
        let mut layer = AddInLayer::new("gh-abc123");

        // Control plane pushes the file content — no fetching
        layer.put_file("/usr/bin/gh", b"#!/bin/sh\necho gh".to_vec(), 0o755);

        assert!(layer.exists("/usr/bin/gh"));
        assert!(layer.exists("/usr/bin")); // parent dir
        assert!(!layer.exists("/usr/bin/rclone"));

        // Content is immediately available — no lazy loading
        let data = layer.read("/usr/bin/gh").unwrap().unwrap();
        assert_eq!(data, b"#!/bin/sh\necho gh");

        let meta = layer.metadata("/usr/bin/gh").unwrap();
        assert_eq!(meta.size, b"#!/bin/sh\necho gh".len() as u64);
        assert_eq!(meta.mode, 0o755);

        let entries = layer.readdir("/usr/bin").unwrap();
        assert!(entries.contains(&"gh".to_string()));
    }

    #[test]
    fn test_writable_layer() {
        let dir = tempfile::tempdir().unwrap();
        let layer = WritableLayer::new(dir.path());

        layer.write_file("/tmp/out.txt", b"hello").unwrap();
        assert!(layer.exists("/tmp/out.txt"));
        assert_eq!(layer.read("/tmp/out.txt").unwrap().unwrap(), b"hello");
    }
}
