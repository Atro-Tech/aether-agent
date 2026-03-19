// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Pinned BPF map definitions for the Æther Agent.
// These maps are shared between the shim-loader (which populates them)
// and the eBPF probes (which read them at runtime in kernel space).

/// Pinned map: credential swap entries.
/// Key: credential_key (32 bytes, null-padded string, e.g. "gh-token")
/// Value: CredentialEntry (real credential bytes + length)
pub const CREDENTIAL_MAP_PATH: &str = "/sys/fs/bpf/aether/credentials";

/// Pinned map: connection routing entries.
/// Key: destination pattern hash (u64)
/// Value: RouteEntry (proxy addr + port + flags)
pub const ROUTE_MAP_PATH: &str = "/sys/fs/bpf/aether/routes";

/// Pinned map: per-PID activation bitmap.
/// Key: pid (u32)
/// Value: addin_id_bitmap (u64) — which add-ins are active for this process
pub const PID_MAP_PATH: &str = "/sys/fs/bpf/aether/pid_addins";

/// Pinned map: Shimmer file I/O scoring entries.
/// Key: inode or path hash (u64)
/// Value: ShimmerEntry (score + action: allow/alert/block)
pub const SHIMMER_MAP_PATH: &str = "/sys/fs/bpf/aether/shimmer_scores";

// ─── Map entry types (mirrored in BPF C code) ───

#[repr(C)]
#[derive(Debug, Clone)]
pub struct CredentialEntry {
    pub real_value: [u8; 256],
    pub len: u32,
}

impl Default for CredentialEntry {
    fn default() -> Self {
        CredentialEntry {
            real_value: [0u8; 256],
            len: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct RouteEntry {
    pub proxy_addr: [u8; 16], // IPv4-mapped-IPv6 or IPv4
    pub proxy_port: u16,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct PidEntry {
    pub addin_bitmap: u64,
}

/// Shimmer score entry: confidence-based file I/O policy.
/// score 0-100: 0 = definitely safe, 100 = definitely malicious.
/// action: 0 = allow, 1 = alert (log + allow), 2 = block.
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct ShimmerEntry {
    pub score: u32,
    pub action: u32, // 0=allow, 1=alert, 2=block
}
