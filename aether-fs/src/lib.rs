// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// AgentFS: the root filesystem for Æther Agent execution contexts.
//
// AgentFS IS the filesystem. Not a volume mount. Not a sidecar.
// The agent boots into AgentFS and sees a normal Linux environment.
//
// Three layers, resolved top-down:
//
//   ┌─────────────────────────────────┐
//   │  Writable Layer (tmpfs)         │  ← all writes land here
//   │  pip install, npm install, etc. │
//   ├─────────────────────────────────┤
//   │  Add-in Layers (lazy CAS)      │  ← files appear when registered
//   │  /usr/bin/gh, /usr/bin/rclone   │
//   ├─────────────────────────────────┤
//   │  Base Template (read-only)      │  ← squashfs or host directory
//   │  /bin, /usr, /lib, /etc, ...    │
//   └─────────────────────────────────┘
//
// Read resolution: writable → add-ins → base (first match wins)
// Write target: always the writable layer
//
// The agent never knows it's on FUSE.

pub mod layer;
pub mod materializer;
pub mod overlay;
pub mod tree;
