// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Materializer: fetches content by CAS address on first access.
// Used by AgentFS to lazily populate files when they're actually read.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub struct Materializer {
    cache_dir: PathBuf,
}

impl Materializer {
    pub fn new(cache_dir: &Path) -> Self {
        Materializer {
            cache_dir: cache_dir.to_path_buf(),
        }
    }

    /// Materialize a file from the content store.
    /// Returns the local cache path where the content now exists.
    ///
    /// If already cached, returns immediately (no fetch).
    pub async fn materialize(
        &self,
        content_address: &str,
        relative_path: &str,
    ) -> Result<PathBuf> {
        let cache_path = self
            .cache_dir
            .join(content_address)
            .join(relative_path);

        // Already cached? Return immediately.
        if cache_path.exists() {
            return Ok(cache_path);
        }

        // Fetch from host via vsock or local CAS
        let data = self
            .fetch_from_host(content_address, relative_path)
            .await?;

        // Write to cache
        if let Some(parent) = cache_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("failed to create cache parent directory")?;
        }
        tokio::fs::write(&cache_path, &data)
            .await
            .with_context(|| format!("failed to write cache file: {}", cache_path.display()))?;

        Ok(cache_path)
    }

    /// Fetch content from the host.
    /// In Firecracker mode: pulls via vsock CAS protocol.
    /// In namespace mode: reads from a local content store directory.
    async fn fetch_from_host(
        &self,
        content_address: &str,
        relative_path: &str,
    ) -> Result<Vec<u8>> {
        // Try local content store first (namespace mode)
        let local_path = Path::new("/var/lib/aether/cas")
            .join(content_address)
            .join(relative_path);

        if local_path.exists() {
            return tokio::fs::read(&local_path)
                .await
                .with_context(|| format!("failed to read from local CAS: {}", local_path.display()));
        }

        // TODO: vsock CAS protocol for Firecracker mode
        anyhow::bail!(
            "content not found: {}:{} (vsock fetch not yet implemented)",
            content_address,
            relative_path
        )
    }
}
