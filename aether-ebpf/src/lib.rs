// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// eBPF probe definitions for Æther Agent.
// Two probe families:
//   - Hallucinator: credential swapping on outbound connections
//   - Shimmer: real-time file I/O monitoring with confidence-based allow/alert/block

pub mod maps;
pub mod probe;
pub mod shimmer;
