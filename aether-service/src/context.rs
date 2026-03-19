// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Agent context file: /run/aether/context.json
//
// This file is the single source of truth for the in-sandbox agent.
// It answers three questions:
//   1. What tools are installed?
//   2. How do I request new tools?
//   3. What can each tool do?
//
// Updated automatically every time an add-in is registered or removed.
// Any language can read it — no SDK needed, just read the JSON file.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::registry::AddInRegistry;

/// Well-known path where the context file lives inside the sandbox.
pub const CONTEXT_FILE_PATH: &str = "/run/aether/context.json";

/// The full context document written to disk.
#[derive(Debug, Clone, Serialize)]
pub struct AgentContext {
    /// Schema version (so agents can handle upgrades).
    pub version: &'static str,

    /// How to request new tools from inside the sandbox.
    pub request_tools: RequestToolsInfo,

    /// All currently installed tools, keyed by command name.
    pub tools: Vec<ToolInfo>,
}

/// Tells the agent how to ask for tools it doesn't have yet.
#[derive(Debug, Clone, Serialize)]
pub struct RequestToolsInfo {
    /// Human-readable explanation.
    pub description: &'static str,

    /// The ttrpc socket path (for agents that speak ttrpc).
    pub ttrpc_socket: &'static str,

    /// The HTTP gateway URL (for agents that speak HTTP).
    pub http_gateway: &'static str,

    /// Or just write a request file here and the agent picks it up.
    pub request_file: &'static str,
}

/// Everything the agent needs to know about one tool.
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    /// The command name (what you type in the shell): "gh", "rclone", etc.
    pub command: String,

    /// Where the binary lives: "/usr/bin/gh".
    pub path: String,

    /// "native" = real binary, "shimmed" = effect-only illusion,
    /// "hybrid" = real binary + credential/network injection.
    pub tool_type: String,

    /// What this tool does.
    pub description: String,

    /// Example invocations.
    pub examples: Vec<String>,

    /// Capability tags: ["git", "pr", "issue", "release"].
    pub capabilities: Vec<String>,

    /// Which add-in package provides this tool.
    pub addin_id: String,

    /// Package version.
    pub version: String,

    /// Does this tool have credential injection (eBPF hallucination)?
    pub has_credential_injection: bool,
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
        let has_creds = addin
            .manifest
            .proxy_rules
            .as_ref()
            .map(|pr| !pr.rules.is_empty())
            .unwrap_or(false);

        for effect in &addin.manifest.effects {
            // Derive the install path from the binaries list, or guess from binary_name.
            let path = addin
                .manifest
                .binaries
                .iter()
                .find(|b| b.path.ends_with(&effect.binary_name))
                .map(|b| format!("/{}", b.path.trim_start_matches('/')))
                .unwrap_or_else(|| format!("/usr/bin/{}", effect.binary_name));

            tools.push(ToolInfo {
                command: effect.binary_name.clone(),
                path,
                tool_type: effect.tool_type.clone(),
                description: if effect.description.is_empty() {
                    addin.manifest.package.description.clone()
                } else {
                    effect.description.clone()
                },
                examples: effect.examples.clone(),
                capabilities: effect.capabilities.clone(),
                addin_id: addin.id.clone(),
                version: addin.version.clone(),
                has_credential_injection: has_creds,
            });
        }
    }

    AgentContext {
        version: "1",
        request_tools: RequestToolsInfo {
            description: "To request a tool that isn't installed, write a JSON request to the request_file path, or call the ttrpc/HTTP endpoint.",
            ttrpc_socket: "unix:///run/aether/agent.sock",
            http_gateway: "http://localhost:1025/v1/addins",
            request_file: "/run/aether/requests/tool_request.json",
        },
        tools,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::AddInRegistry;

    #[test]
    fn test_context_with_no_tools() {
        let dir = tempfile::tempdir().unwrap();
        let registry = AddInRegistry::new(dir.path());
        let ctx = build_context(&registry);

        assert_eq!(ctx.version, "1");
        assert!(ctx.tools.is_empty());
    }

    #[test]
    fn test_context_reflects_registered_tools() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = AddInRegistry::new(dir.path());

        let manifest = br#"
[package]
name = "gh"
version = "2.62.0"
description = "GitHub CLI"

[[effects]]
binary_name = "gh"
shim_library = "/usr/lib/aether/shims/libgh_shim.so"
tool_type = "hybrid"
description = "GitHub CLI with credential injection"
examples = ["gh repo list", "gh pr create --title 'fix'"]
capabilities = ["git", "pr", "issue", "release"]

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
        assert_eq!(tool.command, "gh");
        assert_eq!(tool.path, "/usr/bin/gh");
        assert_eq!(tool.tool_type, "hybrid");
        assert!(tool.has_credential_injection);
        assert!(tool.capabilities.contains(&"pr".to_string()));
        assert_eq!(tool.examples.len(), 2);
    }

    #[test]
    fn test_context_writes_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let registry = AddInRegistry::new(dir.path());

        let ctx_path = dir.path().join("context.json");
        write_context_file_to(&registry, &ctx_path).unwrap();

        let content = std::fs::read_to_string(&ctx_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["version"], "1");
        assert!(parsed["tools"].as_array().unwrap().is_empty());
        assert!(parsed["request_tools"]["ttrpc_socket"].as_str().is_some());
    }
}
