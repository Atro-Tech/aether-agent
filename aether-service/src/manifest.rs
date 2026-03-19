// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Parses per-package TOML manifests into typed Rust structs.
//
// Æther does NOT define what fields an add-in should have beyond the
// minimum needed to operate (package name, version, binary paths).
// Everything else is freeform metadata that Æther passes through to
// the in-sandbox context file. The add-in decides how to describe itself.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Top-level manifest for an add-in package.
#[derive(Debug, Clone, Deserialize, Serialize)]
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

    /// Freeform metadata. The add-in puts whatever it wants here.
    /// Æther passes it through to context.json untouched.
    /// Examples: tool_type, capabilities, examples, description,
    /// auth_method, rate_limits, documentation_url — anything.
    #[serde(default, flatten)]
    pub extra: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    /// Freeform. Æther doesn't interpret this.
    #[serde(default, flatten)]
    pub extra: HashMap<String, toml::Value>,
}

/// An effect: what happens when a binary runs.
/// Only binary_name and shim_library are required by Æther.
/// Everything else is freeform metadata passed to the agent.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Effect {
    /// The command name Æther needs to know about.
    pub binary_name: String,

    /// The LD_PRELOAD library Æther loads.
    pub shim_library: String,

    /// Extra env vars Æther injects.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Everything else: description, tool_type, capabilities,
    /// examples, auth_info, rate_limits — the add-in decides.
    /// Æther passes it all through to context.json.
    #[serde(default, flatten)]
    pub meta: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BinaryEntry {
    pub path: String,
    #[serde(default = "default_source")]
    pub source: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibraryEntry {
    pub path: String,
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "lazy".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyRulesConfig {
    #[serde(default)]
    pub rules: Vec<ProxyRuleEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    fn test_parse_minimal_manifest() {
        let toml = r#"
[package]
name = "gh"
version = "2.62.0"

[[effects]]
binary_name = "gh"
shim_library = "/usr/lib/aether/shims/libgh_shim.so"

[[binaries]]
path = "bin/gh"
"#;
        let m = parse_manifest(toml).unwrap();
        assert_eq!(m.package.name, "gh");
        assert_eq!(m.effects[0].binary_name, "gh");
    }

    #[test]
    fn test_freeform_metadata_passes_through() {
        let toml = r#"
[package]
name = "gh"
version = "2.62.0"
description = "GitHub CLI"
documentation_url = "https://cli.github.com"

[[effects]]
binary_name = "gh"
shim_library = "/usr/lib/aether/shims/libgh_shim.so"
tool_type = "hybrid"
description = "GitHub CLI with cred injection"
examples = ["gh repo list", "gh pr create"]
capabilities = ["git", "pr", "issue"]
rate_limit = "5000/hour"

[[binaries]]
path = "bin/gh"
"#;
        let m = parse_manifest(toml).unwrap();

        // Freeform package fields
        assert!(m.package.extra.contains_key("description"));
        assert!(m.package.extra.contains_key("documentation_url"));

        // Freeform effect fields
        let effect = &m.effects[0];
        assert!(effect.meta.contains_key("tool_type"));
        assert!(effect.meta.contains_key("capabilities"));
        assert!(effect.meta.contains_key("rate_limit"));
        assert!(effect.meta.contains_key("examples"));
    }
}
