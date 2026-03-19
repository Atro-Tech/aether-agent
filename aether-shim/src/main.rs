// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// aether-shim-loader: runs as an OCI StartContainer hook.
//
// This binary executes inside the container namespace, right before execvp().
// It reads all registered add-in manifests and:
//   1. Collects LD_PRELOAD libraries from each effect
//   2. Writes the combined LD_PRELOAD to /run/aether/env (read by bootstrap shim)
//   3. Populates eBPF pinned maps with credential routing entries
//
// Installed at: /usr/libexec/aether/shim-loader
// Invoked by the Æther Agent via OCI hooks during container creation.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use aether_service::manifest::{self, Manifest};

// ─── OCI Hook State (passed on stdin) ───

#[derive(Debug, Deserialize)]
struct OciState {
    #[serde(default)]
    pid: u32,
    #[serde(default)]
    id: String,
}

// ─── CLI Args ───

struct Args {
    manifest_dir: PathBuf,
    env_output_dir: PathBuf,
    ebpf_map_dir: PathBuf,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("aether-shim-loader: error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = parse_args()?;

    // OCI hooks receive container state on stdin
    let state = read_oci_state()?;
    eprintln!(
        "aether-shim-loader: container={} pid={}",
        state.id, state.pid
    );

    // Load all manifests from the manifest directory
    let manifests = load_all_manifests(&args.manifest_dir)?;
    if manifests.is_empty() {
        eprintln!("aether-shim-loader: no manifests found, nothing to do");
        return Ok(());
    }

    // Collect LD_PRELOAD paths and env vars from all effects
    let preloads = collect_preloads(&manifests);
    let env_vars = collect_env_vars(&manifests);

    // Write the combined environment file
    write_env_file(&args.env_output_dir, &preloads, &env_vars)?;

    // Populate eBPF credential maps from proxy rules
    populate_ebpf_maps(&args.ebpf_map_dir, &manifests)?;

    eprintln!(
        "aether-shim-loader: applied {} effects, {} preloads, {} proxy rules",
        manifests.iter().map(|m| m.effects.len()).sum::<usize>(),
        preloads.len(),
        manifests
            .iter()
            .filter_map(|m| m.proxy_rules.as_ref())
            .flat_map(|pr| &pr.rules)
            .count(),
    );

    Ok(())
}

// ─── Args parsing (no clap needed for a hook binary) ───

fn parse_args() -> Result<Args> {
    let cli_args: Vec<String> = std::env::args().collect();

    let manifest_dir = find_arg(&cli_args, "--manifest-dir")
        .unwrap_or_else(|| "/run/aether/manifests".to_string());

    let env_output_dir = find_arg(&cli_args, "--env-dir")
        .unwrap_or_else(|| "/run/aether/env".to_string());

    let ebpf_map_dir = find_arg(&cli_args, "--ebpf-map-dir")
        .unwrap_or_else(|| "/sys/fs/bpf/aether".to_string());

    Ok(Args {
        manifest_dir: PathBuf::from(manifest_dir),
        env_output_dir: PathBuf::from(env_output_dir),
        ebpf_map_dir: PathBuf::from(ebpf_map_dir),
    })
}

fn find_arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

// ─── OCI State ───

fn read_oci_state() -> Result<OciState> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("failed to read OCI state from stdin")?;

    if buf.trim().is_empty() {
        return Ok(OciState {
            pid: 0,
            id: "unknown".to_string(),
        });
    }

    serde_json::from_str(&buf).context("failed to parse OCI state JSON")
}

// ─── Manifest Loading ───

fn load_all_manifests(dir: &Path) -> Result<Vec<Manifest>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut manifests = Vec::new();

    let entries = fs::read_dir(dir).context("failed to read manifest directory")?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read manifest: {}", path.display()))?;

        match manifest::parse_manifest(&content) {
            Ok(m) => manifests.push(m),
            Err(e) => {
                eprintln!(
                    "aether-shim-loader: warning: skipping {}: {e}",
                    path.display()
                );
            }
        }
    }

    Ok(manifests)
}

// ─── Effect Collection ───

fn collect_preloads(manifests: &[Manifest]) -> Vec<String> {
    manifests
        .iter()
        .flat_map(|m| &m.effects)
        .map(|e| e.shim_library.clone())
        .collect()
}

fn collect_env_vars(manifests: &[Manifest]) -> Vec<(String, String)> {
    manifests
        .iter()
        .flat_map(|m| &m.effects)
        .flat_map(|e| e.env.iter())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

// ─── Environment File Output ───
//
// Writes /run/aether/env/ld_preload and /run/aether/env/extra_env.
// The bootstrap LD_PRELOAD shim reads these at process startup.

fn write_env_file(
    env_dir: &Path,
    preloads: &[String],
    env_vars: &[(String, String)],
) -> Result<()> {
    fs::create_dir_all(env_dir).context("failed to create env output directory")?;

    // Write LD_PRELOAD list (colon-separated)
    if !preloads.is_empty() {
        let preload_str = preloads.join(":");
        fs::write(env_dir.join("ld_preload"), &preload_str)
            .context("failed to write ld_preload file")?;
    }

    // Write extra env vars (KEY=VALUE per line)
    if !env_vars.is_empty() {
        let env_str: String = env_vars
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(env_dir.join("extra_env"), &env_str)
            .context("failed to write extra_env file")?;
    }

    Ok(())
}

// ─── eBPF Map Population ───
//
// Writes credential routing entries to pinned BPF map files.
// In production, this uses libbpf to update pinned hash maps.
// For now, we write structured files that the eBPF loader reads.

fn populate_ebpf_maps(ebpf_map_dir: &Path, manifests: &[Manifest]) -> Result<()> {
    let rules: Vec<_> = manifests
        .iter()
        .filter_map(|m| m.proxy_rules.as_ref())
        .flat_map(|pr| &pr.rules)
        .collect();

    if rules.is_empty() {
        return Ok(());
    }

    fs::create_dir_all(ebpf_map_dir).context("failed to create eBPF map directory")?;

    // Write credential routing table for the hallucinator probe
    let cred_entries: Vec<String> = rules
        .iter()
        .map(|r| {
            format!(
                "{}|{}|{}",
                r.match_pattern, r.credential_key, r.target_address
            )
        })
        .collect();

    fs::write(
        ebpf_map_dir.join("credential_routes"),
        cred_entries.join("\n"),
    )
    .context("failed to write credential_routes")?;

    Ok(())
}
