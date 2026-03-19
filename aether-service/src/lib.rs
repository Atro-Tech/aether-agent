// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Æther Agent service crate.
// Provides add-in registration, manifest parsing, and the registry
// that the ttrpc handler and shim-loader both depend on.

pub mod manifest;
pub mod registry;

pub use manifest::Manifest;
pub use registry::{AddInRegistry, RegisteredAddIn};
