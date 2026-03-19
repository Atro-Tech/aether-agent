// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// AddInRegistry: in-memory store for registered add-in packages.
// Writes manifests to /run/aether/manifests/ so the shim-loader can read them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::manifest::{self, Manifest};

/// A registered add-in with its parsed manifest and generated ID.
#[derive(Debug, Clone)]
pub struct RegisteredAddIn {
    pub id: String,
    pub name: String,
    pub version: String,
    pub content_address: String,
    pub manifest: Manifest,
    pub raw_toml: String,
}

/// In-memory registry of all add-in packages.
/// Also persists manifests to disk for the shim-loader to read.
pub struct AddInRegistry {
    addins: HashMap<String, RegisteredAddIn>,
    manifest_dir: PathBuf,
}

impl AddInRegistry {
    pub fn new(manifest_dir: &Path) -> Self {
        AddInRegistry {
            addins: HashMap::new(),
            manifest_dir: manifest_dir.to_path_buf(),
        }
    }

    /// Register a new add-in from raw TOML manifest bytes.
    /// Returns the generated addin_id on success.
    pub fn register(
        &mut self,
        name: &str,
        version: &str,
        manifest_toml: &[u8],
        content_address: &str,
    ) -> Result<String> {
        let toml_str =
            std::str::from_utf8(manifest_toml).context("manifest_toml is not valid UTF-8")?;

        let parsed = manifest::parse_manifest(toml_str)
            .context("failed to parse add-in manifest TOML")?;

        let id = generate_id(name, version);

        let entry = RegisteredAddIn {
            id: id.clone(),
            name: name.to_string(),
            version: version.to_string(),
            content_address: content_address.to_string(),
            manifest: parsed,
            raw_toml: toml_str.to_string(),
        };

        // Write manifest to disk so shim-loader can find it
        self.persist_manifest(&id, toml_str)?;

        self.addins.insert(id.clone(), entry);
        Ok(id)
    }

    pub fn get(&self, addin_id: &str) -> Option<&RegisteredAddIn> {
        self.addins.get(addin_id)
    }

    pub fn list(&self) -> Vec<&RegisteredAddIn> {
        self.addins.values().collect()
    }

    pub fn remove(&mut self, addin_id: &str) -> Option<RegisteredAddIn> {
        let removed = self.addins.remove(addin_id);

        // Clean up on-disk manifest
        if removed.is_some() {
            let path = self.manifest_dir.join(format!("{}.toml", addin_id));
            let _ = std::fs::remove_file(path);
        }

        removed
    }

    fn persist_manifest(&self, addin_id: &str, toml_content: &str) -> Result<()> {
        std::fs::create_dir_all(&self.manifest_dir)
            .context("failed to create manifest directory")?;

        let path = self.manifest_dir.join(format!("{}.toml", addin_id));
        std::fs::write(&path, toml_content)
            .with_context(|| format!("failed to write manifest to {}", path.display()))?;

        Ok(())
    }
}

/// Generate a deterministic add-in ID from name + version.
fn generate_id(name: &str, version: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    hasher.update(b":");
    hasher.update(version.as_bytes());
    let hash = hasher.finalize();
    format!("{}-{}", name, &hex::encode(&hash[..8]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_register_and_get() {
        let dir = tempdir().unwrap();
        let mut reg = AddInRegistry::new(dir.path());

        let manifest = br#"
[package]
name = "gh"
version = "2.62.0"
description = "GitHub CLI"

[[effects]]
binary_name = "gh"
shim_library = "/usr/lib/aether/shims/libgh_shim.so"
"#;

        let id = reg.register("gh", "2.62.0", manifest, "sha256:abc").unwrap();
        assert!(id.starts_with("gh-"));

        let entry = reg.get(&id).unwrap();
        assert_eq!(entry.name, "gh");
        assert_eq!(entry.manifest.effects.len(), 1);

        // Verify on-disk persistence
        let path = dir.path().join(format!("{}.toml", id));
        assert!(path.exists());
    }
}
