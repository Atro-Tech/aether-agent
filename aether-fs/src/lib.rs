// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// AgentFS: the root filesystem for Æther Agent execution contexts.
//
// FUSE IS the filesystem. Not a volume. Not a sidecar.
//
// Three layers, resolved top-down:
//
//   ┌─────────────────────────────────┐
//   │  Writable Layer (tmpfs)         │  ← writes from inside the machine
//   ├─────────────────────────────────┤
//   │  Add-in Layers                  │  ← pushed in by the control plane
//   │  /usr/bin/gh just appears       │    from object storage (S3/R2/GCS)
//   ├─────────────────────────────────┤
//   │  Base Template (read-only)      │  ← the golden image directory
//   └─────────────────────────────────┘
//
// The agent never fetches files. The control plane reads from object
// storage and pushes bytes into AgentFS. Files just appear.
// This is outside-in, not inside-out.

pub mod layer;
pub mod overlay;
pub mod tree;
