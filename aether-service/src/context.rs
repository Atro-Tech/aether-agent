// Context file: /run/aether/context.json
//
// The agent inside reads this to know what tools it has and how
// to ask for more. Updated by the daemon whenever Shimmer registers
// or removes an add-in.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::manifest::Manifest;

pub const CONTEXT_PATH: &str = "/run/aether/context.json";

#[derive(Serialize)]
pub struct AgentContext {
    pub version: &'static str,
    pub request_tools: RequestTools,
    pub tools: Vec<Tool>,
}

#[derive(Serialize)]
pub struct RequestTools {
    pub ttrpc_socket: &'static str,
    pub request_file: &'static str,
}

#[derive(Serialize)]
pub struct Tool {
    pub command: String,
    pub path: String,
    pub addin: String,
    pub version: String,

    /// Everything else the add-in declared. Passed through.
    #[serde(flatten)]
    pub meta: HashMap<String, serde_json::Value>,
}

/// Build context from a list of (addin_id, manifest) pairs.
pub fn build(addins: &[(&str, &Manifest)]) -> AgentContext {
    let mut tools = Vec::new();

    for (addin_id, manifest) in addins {
        for effect in &manifest.effects {
            let path = manifest
                .binaries
                .iter()
                .find(|b| b.path.ends_with(&effect.binary_name))
                .map(|b| format!("/{}", b.path.trim_start_matches('/')))
                .unwrap_or_else(|| format!("/usr/bin/{}", effect.binary_name));

            let meta = effect
                .meta
                .iter()
                .filter_map(|(k, v)| toml_to_json(v).map(|j| (k.clone(), j)))
                .collect();

            tools.push(Tool {
                command: effect.binary_name.clone(),
                path,
                addin: addin_id.to_string(),
                version: manifest.package.version.clone(),
                meta,
            });
        }
    }

    AgentContext {
        version: "1",
        request_tools: RequestTools {
            ttrpc_socket: "unix:///run/aether/agent.sock",
            request_file: "/run/aether/requests/tool_request.json",
        },
        tools,
    }
}

/// Write context.json to disk.
pub fn write(addins: &[(&str, &Manifest)], path: &Path) -> Result<()> {
    let ctx = build(addins);
    let json = serde_json::to_string_pretty(&ctx)
        .context("failed to serialize context")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, json.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))
}

fn toml_to_json(v: &toml::Value) -> Option<serde_json::Value> {
    match v {
        toml::Value::String(s) => Some(serde_json::Value::String(s.clone())),
        toml::Value::Integer(i) => Some(serde_json::json!(*i)),
        toml::Value::Float(f) => Some(serde_json::json!(*f)),
        toml::Value::Boolean(b) => Some(serde_json::Value::Bool(*b)),
        toml::Value::Array(a) => {
            let items: Vec<_> = a.iter().filter_map(toml_to_json).collect();
            Some(serde_json::Value::Array(items))
        }
        toml::Value::Table(t) => {
            let map: serde_json::Map<_, _> = t
                .iter()
                .filter_map(|(k, v)| toml_to_json(v).map(|j| (k.clone(), j)))
                .collect();
            Some(serde_json::Value::Object(map))
        }
        toml::Value::Datetime(d) => Some(serde_json::Value::String(d.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;

    #[test]
    fn builds_context_from_addins() {
        let m = manifest::parse(r#"
[package]
name = "gh"
version = "2.62.0"

[[effects]]
binary_name = "gh"
shim_library = "/usr/lib/aether/shims/libgh_shim.so"
tool_type = "hybrid"

[[binaries]]
path = "usr/bin/gh"
"#).unwrap();

        let ctx = build(&[("gh-abc", &m)]);
        assert_eq!(ctx.tools.len(), 1);
        assert_eq!(ctx.tools[0].command, "gh");
        assert_eq!(ctx.tools[0].path, "/usr/bin/gh");
        assert_eq!(ctx.tools[0].meta["tool_type"], "hybrid");
    }

    #[test]
    fn writes_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("context.json");

        write(&[], &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["version"], "1");
        assert!(v["tools"].as_array().unwrap().is_empty());
    }
}
