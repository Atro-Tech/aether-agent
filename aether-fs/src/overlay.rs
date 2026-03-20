// FUSE overlay for a single directory.
//
// Snapshots the real directory, mounts FUSE over it, serves the
// original files plus any added files. Writes go to a writable
// directory on real ext4 via a bypass fd opened before the mount.
//
// Usage:
//   let overlay = Overlay::new("/usr/bin", "/var/cache/aether/usr-bin");
//   overlay.snapshot()?;       // read real /usr/bin contents
//   overlay.add_lazy("gh", 50_000_000, 0o755);  // file appears, no bytes yet
//   overlay.put("small.sh", b"#!/bin/sh\necho hi", 0o755);  // eager write
//   // mount FUSE over /usr/bin (when fuser is wired in)

use std::collections::HashMap;
use std::os::unix::io::OwnedFd;
use std::path::PathBuf;
use std::sync::RwLock;
use std::{fs, io};

use anyhow::{Context, Result};

/// One file in the overlay.
#[derive(Clone)]
pub struct Entry {
    /// File permissions.
    pub mode: u32,

    /// File content. None = lazy (bytes not yet available).
    pub content: Option<Vec<u8>>,

    /// Expected size. For eager files, this matches content.len().
    /// For lazy files, this is the size reported to stat() before fetch.
    pub size: u64,

    /// True if this file was added (not from the base snapshot).
    pub added: bool,
}

/// FUSE overlay for one directory.
pub struct Overlay {
    /// The directory we mount over (e.g. /usr/bin).
    pub mount_path: PathBuf,

    /// Where writable content is stored on real ext4.
    pub writable_dir: PathBuf,

    /// File descriptor to the real directory, opened before FUSE mount.
    /// Used to bypass FUSE and write directly to ext4.
    pub bypass_fd: Option<OwnedFd>,

    /// All files visible through this overlay.
    /// Key: filename (not full path, just the name within the directory).
    pub files: RwLock<HashMap<String, Entry>>,
}

impl Overlay {
    pub fn new(mount_path: &str, writable_dir: &str) -> Self {
        Overlay {
            mount_path: PathBuf::from(mount_path),
            writable_dir: PathBuf::from(writable_dir),
            bypass_fd: None,
            files: RwLock::new(HashMap::new()),
        }
    }

    /// Snapshot the real directory contents before mounting FUSE.
    /// Opens a bypass fd to the real directory for later writes.
    pub fn snapshot(&mut self) -> Result<()> {
        let path = &self.mount_path;

        if !path.exists() {
            return Ok(());
        }

        // Open bypass fd BEFORE we mount FUSE over this path.
        // This fd points to the real ext4 directory, not the FUSE mount.
        let dir = fs::File::open(path)
            .with_context(|| format!("failed to open bypass fd for {}", path.display()))?;
        self.bypass_fd = Some(OwnedFd::from(dir));

        // Read existing files into the snapshot.
        let mut files = self.files.write().unwrap();

        let entries = fs::read_dir(path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        for entry in entries.flatten() {
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue,
            };

            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Only snapshot regular files (not dirs, symlinks, etc.)
            if !meta.is_file() {
                continue;
            }

            // Read the content eagerly for the base snapshot.
            // These are already on disk, so reading is cheap.
            let content = match fs::read(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let size = content.len() as u64;
            files.insert(
                name,
                Entry {
                    mode: 0o755,
                    content: Some(content),
                    size,
                    added: false,
                },
            );
        }

        // Create the writable directory on real ext4.
        fs::create_dir_all(&self.writable_dir).ok();

        Ok(())
    }

    /// Add a file with content (eager). Immediately available.
    pub fn put(&self, name: &str, content: Vec<u8>, mode: u32) {
        let size = content.len() as u64;
        let mut files = self.files.write().unwrap();
        files.insert(
            name.to_string(),
            Entry {
                mode,
                content: Some(content),
                size,
                added: true,
            },
        );
    }

    /// Add a lazy file. Shows up in listings with the right size and mode,
    /// but content is not available yet. On first read, the FUSE handler
    /// will need to fetch bytes (e.g. signal Shimmer).
    pub fn add_lazy(&self, name: &str, size: u64, mode: u32) {
        let mut files = self.files.write().unwrap();
        files.insert(
            name.to_string(),
            Entry {
                mode,
                content: None, // lazy — no bytes yet
                size,
                added: true,
            },
        );
    }

    /// Fill in the content for a lazy file (called after fetch).
    pub fn fill(&self, name: &str, content: Vec<u8>) {
        let mut files = self.files.write().unwrap();
        if let Some(entry) = files.get_mut(name) {
            entry.size = content.len() as u64;
            entry.content = Some(content);
        }
    }

    /// Remove a file.
    pub fn remove(&self, name: &str) {
        let mut files = self.files.write().unwrap();
        files.remove(name);
    }

    /// Check if a file exists.
    pub fn exists(&self, name: &str) -> bool {
        let files = self.files.read().unwrap();
        files.contains_key(name)
    }

    /// Read file content. Returns None if the file doesn't exist.
    /// Returns Some(None) if the file exists but is lazy (not yet fetched).
    /// Returns Some(Some(bytes)) if content is available.
    pub fn read(&self, name: &str) -> Option<Option<Vec<u8>>> {
        let files = self.files.read().unwrap();
        let entry = files.get(name)?;
        Some(entry.content.clone())
    }

    /// Get file metadata.
    pub fn stat(&self, name: &str) -> Option<(u64, u32)> {
        let files = self.files.read().unwrap();
        let entry = files.get(name)?;
        Some((entry.size, entry.mode))
    }

    /// List all filenames.
    pub fn list(&self) -> Vec<String> {
        let files = self.files.read().unwrap();
        let mut names: Vec<String> = files.keys().cloned().collect();
        names.sort();
        names
    }

    /// Is this file lazy (exists but no content yet)?
    pub fn is_lazy(&self, name: &str) -> bool {
        let files = self.files.read().unwrap();
        match files.get(name) {
            Some(entry) => entry.content.is_none(),
            None => false,
        }
    }

    /// Write content to the real ext4 writable directory via bypass.
    /// This is how cached lazy-fetched files persist across FUSE restarts.
    pub fn write_to_ext4(&self, name: &str, content: &[u8]) -> io::Result<()> {
        let path = self.writable_dir.join(name);
        fs::write(&path, content)
    }
}


impl std::fmt::Debug for Overlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Overlay")
            .field("mount_path", &self.mount_path)
            .field("writable_dir", &self.writable_dir)
            .field("has_bypass_fd", &self.bypass_fd.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reads_real_files() {
        let dir = tempfile::tempdir().unwrap();
        let writable = tempfile::tempdir().unwrap();

        fs::write(dir.path().join("ls"), b"fake-ls").unwrap();
        fs::write(dir.path().join("cat"), b"fake-cat").unwrap();

        let mut overlay = Overlay::new(
            dir.path().to_str().unwrap(),
            writable.path().to_str().unwrap(),
        );
        overlay.snapshot().unwrap();

        assert!(overlay.exists("ls"));
        assert!(overlay.exists("cat"));
        assert!(!overlay.exists("gh"));

        let content = overlay.read("ls").unwrap().unwrap();
        assert_eq!(content, b"fake-ls");
    }

    #[test]
    fn put_adds_eager_file() {
        let overlay = Overlay::new("/nonexistent", "/tmp/test-writable");

        overlay.put("gh", b"gh-binary".to_vec(), 0o755);

        assert!(overlay.exists("gh"));
        let content = overlay.read("gh").unwrap().unwrap();
        assert_eq!(content, b"gh-binary");

        let (size, mode) = overlay.stat("gh").unwrap();
        assert_eq!(size, 9);
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn lazy_file_exists_but_has_no_content() {
        let overlay = Overlay::new("/nonexistent", "/tmp/test-writable");

        overlay.add_lazy("chromium", 2_000_000_000, 0o755);

        assert!(overlay.exists("chromium"));
        assert!(overlay.is_lazy("chromium"));

        // stat reports the declared size
        let (size, _mode) = overlay.stat("chromium").unwrap();
        assert_eq!(size, 2_000_000_000);

        // read returns None (no bytes yet)
        let content = overlay.read("chromium").unwrap();
        assert!(content.is_none());
    }

    #[test]
    fn fill_provides_content_for_lazy_file() {
        let overlay = Overlay::new("/nonexistent", "/tmp/test-writable");

        overlay.add_lazy("gh", 50_000_000, 0o755);
        assert!(overlay.is_lazy("gh"));

        overlay.fill("gh", b"real-gh-binary".to_vec());
        assert!(!overlay.is_lazy("gh"));

        let content = overlay.read("gh").unwrap().unwrap();
        assert_eq!(content, b"real-gh-binary");
    }

    #[test]
    fn remove_makes_file_disappear() {
        let overlay = Overlay::new("/nonexistent", "/tmp/test-writable");

        overlay.put("gh", b"gh".to_vec(), 0o755);
        assert!(overlay.exists("gh"));

        overlay.remove("gh");
        assert!(!overlay.exists("gh"));
    }

    #[test]
    fn list_returns_sorted_names() {
        let overlay = Overlay::new("/nonexistent", "/tmp/test-writable");

        overlay.put("zsh", b"z".to_vec(), 0o755);
        overlay.put("bash", b"b".to_vec(), 0o755);
        overlay.put("gh", b"g".to_vec(), 0o755);

        let names = overlay.list();
        assert_eq!(names, vec!["bash", "gh", "zsh"]);
    }
}
