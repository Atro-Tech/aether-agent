// Parse add-in manifests from TOML.
//
// Æther only cares about a few fields (binary_name, shim_library, env,
// binaries, proxy_rules). Everything else the add-in defines is freeform
// metadata passed through to context.json untouched.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub package: Package,

    #[serde(default)]
    pub effects: Vec<Effect>,

    #[serde(default)]
    pub binaries: Vec<Binary>,

    #[serde(default)]
    pub proxy_rules: Option<ProxyRules>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Package {
    pub name: String,
    pub version: String,

    /// Everything else the add-in wants to say about itself.
    #[serde(default, flatten)]
    pub extra: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Effect {
    pub binary_name: String,
    pub shim_library: String,

    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Freeform metadata. Æther passes it through.
    #[serde(default, flatten)]
    pub meta: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Binary {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyRules {
    #[serde(default)]
    pub rules: Vec<ProxyRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyRule {
    pub match_pattern: String,
    pub credential_key: String,
    #[serde(default)]
    pub target_address: String,
}

pub fn parse(toml_str: &str) -> anyhow::Result<Manifest> {
    Ok(toml::from_str(toml_str)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() {
        let m = parse(r#"
[package]
name = "gh"
version = "2.62.0"

[[effects]]
binary_name = "gh"
shim_library = "/usr/lib/aether/shims/libgh_shim.so"
"#).unwrap();

        assert_eq!(m.package.name, "gh");
        assert_eq!(m.effects[0].binary_name, "gh");
    }

    #[test]
    fn freeform_fields_pass_through() {
        let m = parse(r#"
[package]
name = "gh"
version = "2.62.0"
whatever = "the add-in decides"

[[effects]]
binary_name = "gh"
shim_library = "/usr/lib/aether/shims/libgh_shim.so"
tool_type = "hybrid"
custom_thing = 42
"#).unwrap();

        assert!(m.package.extra.contains_key("whatever"));
        assert!(m.effects[0].meta.contains_key("tool_type"));
        assert!(m.effects[0].meta.contains_key("custom_thing"));
    }
}
