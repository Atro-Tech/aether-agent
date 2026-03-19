// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Materializer: fetches content by CAS address on first access.
//
// When AgentFS gets a read() for an add-in file that hasn't been
// fetched yet, the materializer pulls the content and caches it.
// Subsequent reads hit the cache directly.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub struct Materializer {
    cache_dir: PathBuf,
    cas_dir: PathBuf,
}

impl Materializer {
    /// Create a materializer.
    /// cache_dir: where fetched files are cached locally.
    /// cas_dir: local content-addressable store (namespace mode).
    pub fn new(cache_dir: &Path, cas_dir: &Path) -> Self {
        Materializer {
            cache_dir: cache_dir.to_path_buf(),
            cas_dir: cas_dir.to_path_buf(),
        }
    }

    /// Materialize a file from the CAS.
    /// Returns the local path where the content now exists.
    pub fn materialize(
        &self,
        content_address: &str,
        relative_path: &str,
    ) -> Result<PathBuf> {
        let cache_path = self
            .cache_dir
            .join(content_address)
            .join(relative_path);

        // Already cached
        if cache_path.exists() {
            return Ok(cache_path);
        }

        // Fetch from local CAS
        let data = self.fetch(content_address, relative_path)?;

        // Write to cache
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)
                .context("failed to create cache parent directory")?;
        }
        std::fs::write(&cache_path, &data)
            .with_context(|| format!("failed to write cache: {}", cache_path.display()))?;

        Ok(cache_path)
    }

    /// Read the content of a materialized file (or fetch it first).
    pub fn read_content(
        &self,
        content_address: &str,
        relative_path: &str,
    ) -> Result<Vec<u8>> {
        let cache_path = self.materialize(content_address, relative_path)?;
        std::fs::read(&cache_path)
            .with_context(|| format!("failed to read cached file: {}", cache_path.display()))
    }

    fn fetch(&self, content_address: &str, relative_path: &str) -> Result<Vec<u8>> {
        // Try local CAS directory
        let local = self.cas_dir.join(content_address).join(relative_path);
        if local.exists() {
            return std::fs::read(&local)
                .with_context(|| format!("failed to read CAS: {}", local.display()));
        }

        // TODO: vsock CAS protocol for Firecracker mode
        anyhow::bail!(
            "content not found in CAS: {}:{}",
            content_address,
            relative_path
        )
    }
}
