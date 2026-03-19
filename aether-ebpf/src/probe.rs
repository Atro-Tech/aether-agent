// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Hallucinator probe loader.
//
// The hallucinator attaches to tcp_connect and sendto syscalls.
// For each outbound connection from an activated PID, it checks the
// credential map and swaps placeholder tokens with real credentials.
//
// In production this uses libbpf-rs. This module provides the Rust-side
// loader and map population helpers. The actual eBPF C source is in
// bpf/hallucinator.bpf.c.

use anyhow::{Context, Result};
use std::path::Path;

/// Represents a loaded hallucinator probe.
/// In production, this holds the libbpf Object and Links.
pub struct HallucinatorProbe {
    loaded: bool,
}

impl HallucinatorProbe {
    /// Load and attach the hallucinator eBPF program.
    /// Requires CAP_BPF + CAP_NET_ADMIN.
    pub fn load_and_attach() -> Result<Self> {
        // TODO: Use libbpf-rs to load bpf/hallucinator.bpf.o
        // and attach to kprobe/tcp_connect + kprobe/__sys_sendto.
        //
        // For now, we just ensure the pinned map directory exists.
        std::fs::create_dir_all("/sys/fs/bpf/aether")
            .context("failed to create BPF pin directory")?;

        Ok(HallucinatorProbe { loaded: true })
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Write a credential entry to the pinned credential map.
    pub fn set_credential(key: &str, value: &[u8]) -> Result<()> {
        let key_len = key.len().min(31);
        let val_len = value.len().min(255);
        let mut real_value = [0u8; 256];
        real_value[..val_len].copy_from_slice(&value[..val_len]);

        // TODO: Use libbpf-rs Map::from_pinned_path(CREDENTIAL_MAP_PATH)
        // For now, write to a staging file that the BPF loader picks up.
        let staging_dir = Path::new("/run/aether/ebpf-staging/credentials");
        std::fs::create_dir_all(staging_dir)?;
        std::fs::write(
            staging_dir.join(&key[..key_len]),
            value,
        )?;

        Ok(())
    }

    /// Activate hallucination for a specific PID with an add-in bitmap.
    pub fn activate_for_pid(pid: u32, addin_bitmap: u64) -> Result<()> {
        // TODO: Use libbpf-rs to update PID_MAP_PATH
        let staging_dir = Path::new("/run/aether/ebpf-staging/pids");
        std::fs::create_dir_all(staging_dir)?;
        std::fs::write(
            staging_dir.join(pid.to_string()),
            addin_bitmap.to_le_bytes(),
        )?;

        Ok(())
    }
}
