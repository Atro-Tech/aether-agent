// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Agent context file: /run/aether/context.json
//
// Æther writes this file. It does NOT interpret the contents beyond
// what it needs to operate (binary_name, shim_library, proxy_rules).
// Everything else — descriptions, capabilities, tool types, examples,
// auth methods — is defined by the add-in and passed through verbatim.
//
// The add-in decides how to describe itself. Æther just delivers it.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::registry::AddInRegistry;

/// Well-known path where the context file lives inside the sandbox.
pub const CONTEXT_FILE_PATH: &str = "/run/aether/context.json";

/// The context document. Æther owns the structure; add-ins own the content.
#[derive(Debug, Clone, Serialize)]
pub struct AgentContext {
    /// Schema version.
    pub version: &'static str,

    /// How to request new tools from inside the sandbox.
    pub request_tools: RequestToolsInfo,

    /// All currently installed tools.
    /// Each entry is built from the add-in's manifest — Æther passes
    /// through whatever the add-in declared.
    pub tools: Vec<ToolEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestToolsInfo {
    pub ttrpc_socket: &'static str,
    pub http_gateway: &'static str,
    pub request_file: &'static str,
}

/// One tool entry in the context file.
/// `command` and `path` come from Æther's processing.
/// `meta` is everything the add-in declared — passed through as-is.
#[derive(Debug, Clone, Serialize)]
pub struct ToolEntry {
    /// The command name (from effect.binary_name).
    pub command: String,

    /// Install path (derived from binaries list).
    pub path: String,

    /// Add-in ID and version (Æther-managed).
    pub addin_id: String,
    pub addin_version: String,

    /// Whether this tool has proxy/credential rules (Æther knows this).
    pub has_proxy_rules: bool,

    /// Everything else the add-in declared on the effect.
    /// tool_type, description, capabilities, examples, rate_limits,
    /// auth_method — whatever the add-in put in the manifest.
    /// Æther does not interpret these. It passes them through.
    #[serde(flatten)]
    pub meta: HashMap<String, serde_json::Value>,
}

/// Rebuild and write /run/aether/context.json from the current registry state.
pub fn write_context_file(registry: &AddInRegistry) -> Result<()> {
    write_context_file_to(registry, Path::new(CONTEXT_FILE_PATH))
}

/// Rebuild and write context to a specific path (for testing).
pub fn write_context_file_to(registry: &AddInRegistry, path: &Path) -> Result<()> {
    let context = build_context(registry);

    let json = serde_json::to_string_pretty(&context)
        .context("failed to serialize context to JSON")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context("failed to create context file directory")?;
    }

    std::fs::write(path, json.as_bytes())
        .with_context(|| format!("failed to write context file to {}", path.display()))?;

    Ok(())
}

/// Build the context struct from the registry (no I/O).
pub fn build_context(registry: &AddInRegistry) -> AgentContext {
    let mut tools = Vec::new();

    for addin in registry.list() {
        let has_proxy = addin
            .manifest
            .proxy_rules
            .as_ref()
            .map(|pr| !pr.rules.is_empty())
            .unwrap_or(false);

        for effect in &addin.manifest.effects {
            // Derive install path from the binaries list
            let path = addin
                .manifest
                .binaries
                .iter()
                .find(|b| b.path.ends_with(&effect.binary_name))
                .map(|b| format!("/{}", b.path.trim_start_matches('/')))
                .unwrap_or_else(|| format!("/usr/bin/{}", effect.binary_name));

            // Convert the add-in's freeform TOML metadata to JSON values.
            // Æther doesn't look at these. It just converts the format.
            let meta: HashMap<String, serde_json::Value> = effect
                .meta
                .iter()
                .filter_map(|(k, v)| {
                    toml_value_to_json(v).map(|jv| (k.clone(), jv))
                })
                .collect();

            tools.push(ToolEntry {
                command: effect.binary_name.clone(),
                path,
                addin_id: addin.id.clone(),
                addin_version: addin.version.clone(),
                has_proxy_rules: has_proxy,
                meta,
            });
        }
    }

    AgentContext {
        version: "1",
        request_tools: RequestToolsInfo {
            ttrpc_socket: "unix:///run/aether/agent.sock",
            http_gateway: "http://localhost:1025/v1/addins",
            request_file: "/run/aether/requests/tool_request.json",
        },
        tools,
    }
}

/// Convert a TOML value to a JSON value.
fn toml_value_to_json(v: &toml::Value) -> Option<serde_json::Value> {
    match v {
        toml::Value::String(s) => Some(serde_json::Value::String(s.clone())),
        toml::Value::Integer(i) => Some(serde_json::json!(*i)),
        toml::Value::Float(f) => Some(serde_json::json!(*f)),
        toml::Value::Boolean(b) => Some(serde_json::Value::Bool(*b)),
        toml::Value::Array(arr) => {
            let items: Vec<serde_json::Value> = arr
                .iter()
                .filter_map(toml_value_to_json)
                .collect();
            Some(serde_json::Value::Array(items))
        }
        toml::Value::Table(t) => {
            let map: serde_json::Map<String, serde_json::Value> = t
                .iter()
                .filter_map(|(k, v)| toml_value_to_json(v).map(|jv| (k.clone(), jv)))
                .collect();
            Some(serde_json::Value::Object(map))
        }
        toml::Value::Datetime(d) => Some(serde_json::Value::String(d.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::AddInRegistry;

    #[test]
    fn test_context_empty() {
        let dir = tempfile::tempdir().unwrap();
        let registry = AddInRegistry::new(dir.path());
        let ctx = build_context(&registry);

        assert_eq!(ctx.version, "1");
        assert!(ctx.tools.is_empty());
    }

    #[test]
    fn test_context_passes_through_addin_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = AddInRegistry::new(dir.path());

        // The add-in defines its own metadata — Æther doesn't interpret it
        let manifest = br#"
[package]
name = "gh"
version = "2.62.0"

[[effects]]
binary_name = "gh"
shim_library = "/usr/lib/aether/shims/libgh_shim.so"
tool_type = "hybrid"
description = "GitHub CLI with cred injection"
examples = ["gh repo list", "gh pr create"]
capabilities = ["git", "pr", "issue"]
rate_limit = "5000/hour"
custom_field = "add-ins can put anything here"

[[binaries]]
path = "usr/bin/gh"

[proxy_rules]
[[proxy_rules.rules]]
match_pattern = "api.github.com"
credential_key = "gh-token"
"#;

        registry.register("gh", "2.62.0", manifest, "sha256:abc").unwrap();
        let ctx = build_context(&registry);

        assert_eq!(ctx.tools.len(), 1);
        let tool = &ctx.tools[0];

        // Æther-managed fields
        assert_eq!(tool.command, "gh");
        assert_eq!(tool.path, "/usr/bin/gh");
        assert!(tool.has_proxy_rules);

        // Add-in-defined fields — passed through verbatim
        assert_eq!(tool.meta["tool_type"], "hybrid");
        assert_eq!(tool.meta["description"], "GitHub CLI with cred injection");
        assert_eq!(tool.meta["rate_limit"], "5000/hour");
        assert_eq!(tool.meta["custom_field"], "add-ins can put anything here");

        // Array fields
        let caps = tool.meta["capabilities"].as_array().unwrap();
        assert_eq!(caps.len(), 3);
    }

    #[test]
    fn test_context_writes_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let registry = AddInRegistry::new(dir.path());

        let ctx_path = dir.path().join("context.json");
        write_context_file_to(&registry, &ctx_path).unwrap();

        let content = std::fs::read_to_string(&ctx_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["version"], "1");
        assert!(parsed["request_tools"]["ttrpc_socket"].as_str().is_some());
    }
}
