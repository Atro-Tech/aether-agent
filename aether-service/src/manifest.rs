// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Parses per-package TOML manifests into typed Rust structs.
// Each manifest declares a package's effects (shims), binaries, and proxy rules.

use serde::Deserialize;
use std::collections::HashMap;

/// Top-level manifest for an add-in package.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub package: PackageInfo,

    #[serde(default)]
    pub effects: Vec<Effect>,

    #[serde(default)]
    pub binaries: Vec<BinaryEntry>,

    #[serde(default)]
    pub libraries: Vec<LibraryEntry>,

    #[serde(default)]
    pub proxy_rules: Option<ProxyRulesConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub content_address: String,
}

/// An effect is the core illusion unit: what happens when a binary runs.
/// Each effect maps a binary name to an LD_PRELOAD shim and extra env vars.
#[derive(Debug, Clone, Deserialize)]
pub struct Effect {
    pub binary_name: String,
    pub shim_library: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BinaryEntry {
    pub path: String,
    #[serde(default = "default_source")]
    pub source: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryEntry {
    pub path: String,
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "lazy".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxyRulesConfig {
    #[serde(default)]
    pub rules: Vec<ProxyRuleEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxyRuleEntry {
    pub match_pattern: String,
    pub credential_key: String,
    #[serde(default)]
    pub target_address: String,
}

/// Parse a TOML manifest string into a Manifest struct.
pub fn parse_manifest(toml_content: &str) -> anyhow::Result<Manifest> {
    let manifest: Manifest = toml::from_str(toml_content)?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest() {
        let toml = r#"
[package]
name = "gh"
version = "2.62.0"
description = "GitHub CLI"
content_address = "sha256:abc123"

[[effects]]
binary_name = "gh"
shim_library = "/usr/lib/aether/shims/libgh_shim.so"

[effects.env]
GITHUB_TOKEN_SOURCE = "ebpf:gh-token"

[[binaries]]
path = "bin/gh"
source = "lazy"
"#;
        let m = parse_manifest(toml).unwrap();
        assert_eq!(m.package.name, "gh");
        assert_eq!(m.effects.len(), 1);
        assert_eq!(m.effects[0].binary_name, "gh");
        assert_eq!(m.binaries[0].path, "bin/gh");
    }
}
