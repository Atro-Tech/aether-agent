// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// FUSE filesystem operations for AgentFS.
//
// AgentFS presents package binaries and libraries as regular files.
// On first read, the file is lazily materialized from the CAS.
// getattr/lookup return immediately (sizes come from the manifest).
//
// In production, this implements the `fuser::Filesystem` trait.
// This module defines the data structures and logic; the actual FUSE
// mount happens in the agent's main.rs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Represents a virtual file in the AgentFS.
#[derive(Debug, Clone)]
pub struct VirtualFile {
    /// Inode number
    pub ino: u64,
    /// Display path (e.g. "bin/gh")
    pub path: String,
    /// Content address for lazy fetch
    pub content_address: String,
    /// Expected file size (from manifest, 0 if unknown)
    pub size: u64,
    /// File mode (permissions)
    pub mode: u32,
    /// Whether this file has been materialized to local cache
    pub materialized: bool,
    /// Local cache path (set after materialization)
    pub cache_path: Option<PathBuf>,
}

/// The AgentFS path index: maps virtual paths to their metadata.
/// Populated when add-ins are registered; entries exist before files are fetched.
pub struct AgentFs {
    /// Virtual path -> file entry
    pub path_index: Arc<RwLock<HashMap<String, VirtualFile>>>,
    /// Next inode number
    next_ino: u64,
    /// Local cache directory for materialized files
    pub cache_dir: PathBuf,
}

impl AgentFs {
    pub fn new(cache_dir: PathBuf) -> Self {
        AgentFs {
            path_index: Arc::new(RwLock::new(HashMap::new())),
            next_ino: 2, // inode 1 is root
            cache_dir,
        }
    }

    /// Register files from an add-in manifest into the virtual filesystem.
    pub async fn register_addin_files(
        &mut self,
        content_address: &str,
        binaries: &[String],
        libraries: &[String],
    ) {
        let mut index = self.path_index.write().await;

        for path in binaries {
            let ino = self.next_ino;
            self.next_ino += 1;
            index.insert(
                path.clone(),
                VirtualFile {
                    ino,
                    path: path.clone(),
                    content_address: content_address.to_string(),
                    size: 0,
                    mode: 0o755, // executable
                    materialized: false,
                    cache_path: None,
                },
            );
        }

        for path in libraries {
            let ino = self.next_ino;
            self.next_ino += 1;
            index.insert(
                path.clone(),
                VirtualFile {
                    ino,
                    path: path.clone(),
                    content_address: content_address.to_string(),
                    size: 0,
                    mode: 0o644, // shared library
                    materialized: false,
                    cache_path: None,
                },
            );
        }
    }

    /// Mark a file as materialized (called after successful fetch).
    pub async fn mark_materialized(&self, path: &str, cache_path: PathBuf, size: u64) {
        let mut index = self.path_index.write().await;
        if let Some(entry) = index.get_mut(path) {
            entry.materialized = true;
            entry.cache_path = Some(cache_path);
            entry.size = size;
        }
    }
}
