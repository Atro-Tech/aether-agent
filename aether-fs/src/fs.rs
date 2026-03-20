// The filesystem. Three layers stacked.
//
// Read: check writable, then add-ins (newest first), then base.
// Write: always goes to the writable layer.
// Readdir: merge all layers, deduplicate.
//
// Shimmer mutates the add-in layers from outside via ttrpc.
// Processes inside see a normal Linux filesystem.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

// ─── A file ───

#[derive(Clone)]
pub struct File {
    pub content: Vec<u8>,
    pub mode: u32,
}

// ─── The filesystem ───

pub struct AgentFs {
    /// The golden image. Read-only directory on the host.
    base_dir: PathBuf,

    /// Files pushed in by Shimmer. Keyed by absolute path.
    /// Protected by RwLock so Shimmer can push files while
    /// processes inside are reading.
    addin_files: RwLock<HashMap<String, File>>,

    /// Writes from inside the machine.
    writable_dir: PathBuf,
}

impl std::fmt::Debug for AgentFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentFs")
            .field("base_dir", &self.base_dir)
            .field("writable_dir", &self.writable_dir)
            .finish()
    }
}

impl AgentFs {
    pub fn new(base_dir: &Path, writable_dir: &Path) -> Self {
        std::fs::create_dir_all(writable_dir).ok();
        AgentFs {
            base_dir: base_dir.to_path_buf(),
            addin_files: RwLock::new(HashMap::new()),
            writable_dir: writable_dir.to_path_buf(),
        }
    }

    // ─── Called by Shimmer (from outside, via ttrpc) ───

    /// Put a file into the filesystem. It appears immediately.
    pub fn put(&self, path: &str, content: Vec<u8>, mode: u32) {
        let mut files = self.addin_files.write().unwrap();
        files.insert(path.to_string(), File { content, mode });
    }

    /// Remove a file that was previously put.
    pub fn remove(&self, path: &str) {
        let mut files = self.addin_files.write().unwrap();
        files.remove(path);
    }

    /// List all files Shimmer has put in.
    pub fn list_addin_files(&self) -> Vec<String> {
        let files = self.addin_files.read().unwrap();
        files.keys().cloned().collect()
    }

    // ─── Called by FUSE (from inside, by processes) ───

    /// Does this path exist?
    pub fn exists(&self, path: &str) -> bool {
        // Writable layer
        if self.writable_path(path).exists() {
            return true;
        }

        // Add-in layer
        if self.addin_exists(path) {
            return true;
        }

        // Base layer
        self.base_path(path).exists()
    }

    /// Read a file.
    pub fn read(&self, path: &str) -> Option<io::Result<Vec<u8>>> {
        // Writable layer first
        let wp = self.writable_path(path);
        if wp.is_file() {
            return Some(std::fs::read(&wp));
        }

        // Add-in layer
        if let Some(file) = self.addin_read(path) {
            return Some(Ok(file.content));
        }

        // Base layer
        let bp = self.base_path(path);
        if bp.is_file() {
            return Some(std::fs::read(&bp));
        }

        None
    }

    /// Is this path a directory?
    pub fn is_dir(&self, path: &str) -> bool {
        if self.writable_path(path).is_dir() {
            return true;
        }

        if self.addin_is_dir(path) {
            return true;
        }

        self.base_path(path).is_dir()
    }

    /// List directory entries. Merges all layers.
    pub fn readdir(&self, path: &str) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut entries = Vec::new();

        // Writable
        if let Ok(rd) = std::fs::read_dir(self.writable_path(path)) {
            for e in rd.flatten() {
                if let Ok(name) = e.file_name().into_string() {
                    if seen.insert(name.clone()) {
                        entries.push(name);
                    }
                }
            }
        }

        // Add-ins
        {
            let files = self.addin_files.read().unwrap();
            let prefix = if path == "/" {
                "/".to_string()
            } else {
                format!("{}/", path.trim_end_matches('/'))
            };

            for key in files.keys() {
                if !key.starts_with(&prefix) {
                    continue;
                }
                let rest = &key[prefix.len()..];
                // Direct child only (no more slashes)
                if let Some(name) = rest.split('/').next() {
                    if !name.is_empty() && seen.insert(name.to_string()) {
                        entries.push(name.to_string());
                    }
                }
            }
        }

        // Base
        if let Ok(rd) = std::fs::read_dir(self.base_path(path)) {
            for e in rd.flatten() {
                if let Ok(name) = e.file_name().into_string() {
                    if seen.insert(name.clone()) {
                        entries.push(name);
                    }
                }
            }
        }

        entries.sort();
        entries
    }

    /// File size.
    pub fn size(&self, path: &str) -> Option<u64> {
        let wp = self.writable_path(path);
        if wp.is_file() {
            return std::fs::metadata(&wp).ok().map(|m| m.len());
        }

        if let Some(file) = self.addin_read(path) {
            return Some(file.content.len() as u64);
        }

        let bp = self.base_path(path);
        std::fs::metadata(&bp).ok().map(|m| m.len())
    }

    /// File mode.
    pub fn mode(&self, path: &str) -> Option<u32> {
        if self.writable_path(path).exists() {
            return Some(0o644);
        }

        if let Some(file) = self.addin_read(path) {
            return Some(file.mode);
        }

        if self.base_path(path).exists() {
            return Some(0o644);
        }

        None
    }

    // ─── Called by processes inside (writes) ───

    /// Write a file. Always goes to the writable layer.
    pub fn write(&self, path: &str, data: &[u8]) -> io::Result<()> {
        let wp = self.writable_path(path);
        if let Some(parent) = wp.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&wp, data)
    }

    /// Create a directory. Writable layer.
    pub fn mkdir(&self, path: &str) -> io::Result<()> {
        std::fs::create_dir_all(self.writable_path(path))
    }

    // ─── Internal helpers ───

    fn base_path(&self, path: &str) -> PathBuf {
        self.base_dir.join(path.trim_start_matches('/'))
    }

    fn writable_path(&self, path: &str) -> PathBuf {
        self.writable_dir.join(path.trim_start_matches('/'))
    }

    fn addin_exists(&self, path: &str) -> bool {
        let files = self.addin_files.read().unwrap();
        if files.contains_key(path) {
            return true;
        }
        // Check if any file has this as a parent directory
        let prefix = format!("{}/", path.trim_end_matches('/'));
        files.keys().any(|k| k.starts_with(&prefix))
    }

    fn addin_is_dir(&self, path: &str) -> bool {
        let files = self.addin_files.read().unwrap();
        let prefix = format!("{}/", path.trim_end_matches('/'));
        files.keys().any(|k| k.starts_with(&prefix))
    }

    fn addin_read(&self, path: &str) -> Option<File> {
        let files = self.addin_files.read().unwrap();
        files.get(path).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, AgentFs) {
        let base = tempfile::tempdir().unwrap();
        let writable = tempfile::tempdir().unwrap();

        std::fs::create_dir_all(base.path().join("usr/bin")).unwrap();
        std::fs::create_dir_all(base.path().join("etc")).unwrap();
        std::fs::write(base.path().join("usr/bin/ls"), b"real-ls").unwrap();
        std::fs::write(base.path().join("etc/hostname"), b"aether").unwrap();

        let fs = AgentFs::new(base.path(), writable.path());
        (base, writable, fs)
    }

    #[test]
    fn reads_base_files() {
        let (_b, _w, fs) = setup();

        assert!(fs.exists("/usr/bin/ls"));
        assert_eq!(fs.read("/usr/bin/ls").unwrap().unwrap(), b"real-ls");
    }

    #[test]
    fn shimmer_puts_file_it_appears() {
        let (_b, _w, fs) = setup();

        assert!(!fs.exists("/usr/bin/gh"));

        fs.put("/usr/bin/gh", b"gh-binary".to_vec(), 0o755);

        assert!(fs.exists("/usr/bin/gh"));
        assert_eq!(fs.read("/usr/bin/gh").unwrap().unwrap(), b"gh-binary");
        assert_eq!(fs.mode("/usr/bin/gh"), Some(0o755));
    }

    #[test]
    fn shimmer_removes_file_it_disappears() {
        let (_b, _w, fs) = setup();

        fs.put("/usr/bin/gh", b"gh-binary".to_vec(), 0o755);
        assert!(fs.exists("/usr/bin/gh"));

        fs.remove("/usr/bin/gh");
        assert!(!fs.exists("/usr/bin/gh"));
    }

    #[test]
    fn readdir_merges_all_layers() {
        let (_b, _w, fs) = setup();

        // Base has /usr/bin/ls
        // Shimmer puts /usr/bin/gh
        // Process writes /usr/bin/my-tool
        fs.put("/usr/bin/gh", b"gh".to_vec(), 0o755);
        fs.write("/usr/bin/my-tool", b"custom").unwrap();

        let entries = fs.readdir("/usr/bin");
        assert!(entries.contains(&"ls".to_string()));
        assert!(entries.contains(&"gh".to_string()));
        assert!(entries.contains(&"my-tool".to_string()));
    }

    #[test]
    fn writable_overrides_base() {
        let (_b, _w, fs) = setup();

        assert_eq!(fs.read("/etc/hostname").unwrap().unwrap(), b"aether");

        fs.write("/etc/hostname", b"custom").unwrap();

        assert_eq!(fs.read("/etc/hostname").unwrap().unwrap(), b"custom");
    }

    #[test]
    fn addin_overrides_base() {
        let (_b, _w, fs) = setup();

        assert_eq!(fs.read("/usr/bin/ls").unwrap().unwrap(), b"real-ls");

        fs.put("/usr/bin/ls", b"shimmed-ls".to_vec(), 0o755);

        assert_eq!(fs.read("/usr/bin/ls").unwrap().unwrap(), b"shimmed-ls");
    }

    #[test]
    fn writable_overrides_addin() {
        let (_b, _w, fs) = setup();

        fs.put("/usr/bin/gh", b"shimmer-gh".to_vec(), 0o755);
        fs.write("/usr/bin/gh", b"local-gh").unwrap();

        assert_eq!(fs.read("/usr/bin/gh").unwrap().unwrap(), b"local-gh");
    }

    #[test]
    fn addin_dirs_exist() {
        let (_b, _w, fs) = setup();

        fs.put("/opt/tools/bin/mytool", b"tool".to_vec(), 0o755);

        assert!(fs.is_dir("/opt/tools/bin"));
        assert!(fs.is_dir("/opt/tools"));
        assert!(fs.is_dir("/opt"));
    }
}
