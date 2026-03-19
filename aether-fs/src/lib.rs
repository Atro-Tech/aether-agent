// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// AgentFS: FUSE filesystem for the Æther Agent.
//
// Two modes:
//   1. Lazy materialization: package binaries/libraries appear as regular files
//      but are fetched from the CAS (content-addressable store) on first read.
//   2. Writable overlay: dynamic package installs (pip, npm, brew) write to a
//      tmpfs-backed overlay. Useful files are extracted at end, rest discarded.

pub mod fuse_ops;
pub mod materializer;
pub mod overlay;
