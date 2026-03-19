// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Filesystem layers. AgentFS stacks three layer types into a unified tree.
// Each layer can answer "does this path exist?" and "give me its content."
//
// Resolution order: Writable → AddIn (newest first) → Base
// First layer that has the path wins.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ─── File metadata (shared across all layers) ───

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

/// A filesystem layer that can be stacked.
/// Layers are asked "do you have this path?" in priority order.
pub trait Layer: Send + Sync {
    /// Check if this layer has the given path.
    fn exists(&self, path: &str) -> bool;

    /// Get metadata for a path. Returns None if this layer doesn't have it.
    fn metadata(&self, path: &str) -> Option<FileMeta>;

    /// Read file content. Returns None if this layer doesn't have it.
    /// For lazy layers, this may trigger a fetch.
    fn read(&self, path: &str) -> Option<io::Result<Vec<u8>>>;

    /// List entries in a directory. Returns None if this layer doesn't have it.
    fn readdir(&self, path: &str) -> Option<Vec<String>>;

    /// Human-readable name for logging.
    fn name(&self) -> &str;
}

// ─── BaseLayer: the read-only template ───

/// The bottom layer. A real directory on disk (or a mounted squashfs).
/// Contains the base Linux environment: /bin, /usr, /lib, /etc, etc.
pub struct BaseLayer {
    /// Root directory of the base template on the host.
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
        let host = self.host_path(path);
        let meta = std::fs::metadata(&host).ok()?;

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
        let host = self.host_path(path);
        let entries = std::fs::read_dir(&host).ok()?;

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

// ─── AddInLayer: files that appear when a package is registered ───

/// A layer backed by lazily-materialized CAS content.
/// Files "exist" as soon as the add-in is registered (metadata is known),
/// but content is fetched on first read.
pub struct AddInLayer {
    /// Add-in ID (e.g. "gh-a1b2c3d4")
    pub addin_id: String,

    /// Paths this add-in provides, mapped to their metadata.
    /// Key is the absolute path as the agent sees it: "/usr/bin/gh"
    pub files: HashMap<String, AddInFile>,

    /// Directories this add-in creates (derived from file paths).
    pub dirs: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct AddInFile {
    pub meta: FileMeta,
    pub content_address: String,
    /// Path within the CAS package (e.g. "bin/gh")
    pub cas_relative_path: String,
    /// Local cache path (populated after first read)
    pub cached_content: Option<PathBuf>,
}

impl AddInLayer {
    pub fn new(addin_id: &str) -> Self {
        AddInLayer {
            addin_id: addin_id.to_string(),
            files: HashMap::new(),
            dirs: HashMap::new(),
        }
    }

    /// Register a file at an absolute path in the virtual filesystem.
    /// e.g. install_path="/usr/bin/gh", cas_path="bin/gh"
    pub fn add_file(
        &mut self,
        install_path: &str,
        cas_path: &str,
        content_address: &str,
        mode: u32,
    ) {
        self.files.insert(
            install_path.to_string(),
            AddInFile {
                meta: FileMeta::file(0, mode),
                content_address: content_address.to_string(),
                cas_relative_path: cas_path.to_string(),
                cached_content: None,
            },
        );

        // Register parent directories so readdir works
        self.register_parent_dirs(install_path);
    }

    fn register_parent_dirs(&mut self, file_path: &str) {
        let path = Path::new(file_path);
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if let Some(parent) = path.parent() {
            let parent_str = parent.to_string_lossy().to_string();
            self.dirs
                .entry(parent_str.clone())
                .or_default()
                .push(filename);

            // Recurse up to register intermediate directories
            if parent_str != "/" && !parent_str.is_empty() {
                let dir_name = parent
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                if let Some(grandparent) = parent.parent() {
                    let gp_str = grandparent.to_string_lossy().to_string();
                    let entries = self.dirs.entry(gp_str).or_default();
                    if !entries.contains(&dir_name) {
                        entries.push(dir_name);
                    }
                }
            }
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

        // If already cached locally, read from cache
        if let Some(ref cache_path) = file.cached_content {
            return Some(std::fs::read(cache_path));
        }

        // Not cached yet — materializer needs to fetch it.
        // Return a placeholder error. The FUSE handler will call the
        // materializer and retry.
        Some(Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "needs materialization: {}:{}",
                file.content_address, file.cas_relative_path
            ),
        )))
    }

    fn readdir(&self, path: &str) -> Option<Vec<String>> {
        self.dirs.get(path).cloned()
    }

    fn name(&self) -> &str {
        &self.addin_id
    }
}

// ─── WritableLayer: captures all writes ───

/// The top layer. All writes land here.
/// Backed by a tmpfs directory so nothing persists unless extracted.
pub struct WritableLayer {
    /// Root directory for writable content (tmpfs mount point)
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

    /// Write a file (called by FUSE write handler).
    pub fn write_file(&self, path: &str, data: &[u8]) -> io::Result<()> {
        let host = self.host_path(path);
        if let Some(parent) = host.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&host, data)
    }

    /// Create a directory (called by FUSE mkdir handler).
    pub fn mkdir(&self, path: &str) -> io::Result<()> {
        std::fs::create_dir_all(self.host_path(path))
    }

    /// Delete a file or directory (marks as deleted in the overlay).
    pub fn remove(&self, path: &str) -> io::Result<()> {
        let host = self.host_path(path);
        if host.is_dir() {
            std::fs::remove_dir_all(&host)
        } else {
            std::fs::remove_file(&host)
        }
    }

    /// List all written paths (for artifact extraction at task end).
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
        let host = self.host_path(path);
        let meta = std::fs::metadata(&host).ok()?;

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
        let host = self.host_path(path);
        let entries = std::fs::read_dir(&host).ok()?;

        let names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();

        if names.is_empty() {
            return None;
        }

        Some(names)
    }

    fn name(&self) -> &str {
        "writable"
    }
}

/// Recursively collect all file paths relative to root.
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
    fn test_base_layer_reads_host_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("usr/bin")).unwrap();
        fs::write(dir.path().join("usr/bin/ls"), b"fake-ls").unwrap();

        let base = BaseLayer::new(dir.path());
        assert!(base.exists("/usr/bin/ls"));
        assert!(!base.exists("/usr/bin/gh"));

        let data = base.read("/usr/bin/ls").unwrap().unwrap();
        assert_eq!(data, b"fake-ls");

        let entries = base.readdir("/usr/bin").unwrap();
        assert!(entries.contains(&"ls".to_string()));
    }

    #[test]
    fn test_addin_layer_files_appear_at_real_paths() {
        let mut layer = AddInLayer::new("gh-abc123");
        layer.add_file("/usr/bin/gh", "bin/gh", "sha256:abc", 0o755);

        assert!(layer.exists("/usr/bin/gh"));
        assert!(layer.exists("/usr/bin")); // parent dir exists too
        assert!(!layer.exists("/usr/bin/rclone"));

        let meta = layer.metadata("/usr/bin/gh").unwrap();
        assert!(!meta.is_dir);
        assert_eq!(meta.mode, 0o755);

        let dir_entries = layer.readdir("/usr/bin").unwrap();
        assert!(dir_entries.contains(&"gh".to_string()));
    }

    #[test]
    fn test_writable_layer_captures_writes() {
        let dir = tempfile::tempdir().unwrap();
        let layer = WritableLayer::new(dir.path());

        layer.write_file("/etc/config.toml", b"key = 1").unwrap();
        assert!(layer.exists("/etc/config.toml"));

        let data = layer.read("/etc/config.toml").unwrap().unwrap();
        assert_eq!(data, b"key = 1");

        let paths = layer.written_paths();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/etc/config.toml"));
    }
}
