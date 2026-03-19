# Æther Agent

An Agentic Operating Environment (AOE) that runs AI tool-use in isolated contexts with credential hallucination, lazy binary materialization, and real-time file I/O monitoring.

Forked from the [Kata Containers](https://github.com/kata-containers/kata-containers) Rust agent. All Kata functionality is preserved.

## Architecture

```
                    Control Plane (Go / Elixir)
                           |
                      ttrpc / vsock
                           |
                    ┌──────┴──────┐
                    │ Æther Agent │
                    │  (Rust)     │
                    └──────┬──────┘
                           │
          ┌────────────────┼────────────────┐
          │                │                │
    ┌─────┴─────┐   ┌─────┴─────┐   ┌─────┴─────┐
    │  Effects   │   │  AgentFS  │   │  Shimmer   │
    │ LD_PRELOAD │   │   FUSE    │   │   eBPF     │
    │  + eBPF    │   │  lazy CAS │   │  file I/O  │
    └───────────┘   └───────────┘   └───────────┘
```

### Core Concepts

**Effects** are the core illusion unit. Each add-in package declares effects that map binary names to LD_PRELOAD shims and eBPF credential routing rules. When a container runs `gh`, the effect swaps a placeholder token for a real GitHub credential — the process never sees the real secret.

**Layer 0** is host-native by default: Linux namespaces + bubblewrap for speed and native compatibility. Pluggable backends (Firecracker/Kata, gVisor) are selectable per add-in for stronger isolation.

**AgentFS** is a FUSE filesystem that lazily materializes package binaries on first access. A 2GB Playwright browser bundle is never fetched unless Playwright actually runs. Writable overlays support dynamic `pip install` / `npm install` with artifact extraction.

**Shimmer** monitors file I/O in real-time using eBPF (Falco/Tetragon style). Each file access gets a confidence score (0-100) with allow/alert/block actions.

## Quick Start

```bash
# Build the agent
cargo build --release

# Build the shim-loader
cargo build --release -p aether-shim

# Register an add-in (example: GitHub CLI)
# via ttrpc client or the Go/Elixir control-plane library
```

## Add-In Packages

Each package has a TOML manifest in `packages/`:

| Package | Description | Credential Key |
|---------|-------------|----------------|
| `gh` | GitHub CLI | `gh-token` |
| `rclone-dropbox` | rclone for Dropbox | `dropbox-oauth` |
| `playwright` | Browser automation | (none) |
| `python-excel` | Python + openpyxl | (none) |
| `imap-scraper` | IMAP email scraper | `imap-password` |

### Manifest Format

```toml
[package]
name = "gh"
version = "2.62.0"
description = "GitHub CLI with credential hallucination"
content_address = "sha256:..."

[[effects]]
binary_name = "gh"
shim_library = "/usr/lib/aether/shims/libgh_shim.so"

[effects.env]
GITHUB_TOKEN_SOURCE = "ebpf:gh-token"

[[binaries]]
path = "bin/gh"
source = "lazy"

[proxy_rules]
[[proxy_rules.rules]]
match_pattern = "api.github.com"
credential_key = "gh-token"
```

## API (aether.proto)

```protobuf
service AetherAgent {
    rpc RegisterAddIn(AddInRequest) returns (AddInResponse);
    rpc MaterializePath(MaterializeRequest) returns (MaterializeResponse);
    rpc UpdateProxyRules(ProxyRules) returns (Empty);
    rpc GetManifest(GetManifestRequest) returns (ManifestResponse);
    rpc StreamLogs(LogRequest) returns (stream LogResponse);
}
```

## Project Structure

```
aether-agent/
├── agent/              # Forked Kata agent (modified: rpc.rs, sandbox.rs, main.rs)
├── protocols/          # Proto definitions + codegen (aether.proto added)
├── aether-service/     # Add-in registry, manifest parsing, context file
├── aether-ebpf/        # Hallucinator probe + Shimmer file I/O monitor
├── aether-fs/          # AgentFS — the root filesystem (base + add-in + writable layers)
├── packages/           # Default add-in manifests (5 packages)
└── clients/            # Control-plane clients (Go + Elixir)
```

## How Effects Work

1. Control plane calls `RegisterAddIn` with a package manifest
2. Agent stores the manifest in its in-memory registry
3. Agent updates `/run/aether/context.json` (so the in-sandbox agent knows what tools exist)
4. Container is created — the agent applies effects inline:
   - Injects `LD_PRELOAD` with shim libraries directly into the OCI process env
   - Injects extra env vars from each effect
   - Populates eBPF pinned maps with credential routing rules
5. Container process starts with effects already active in its environment
6. Outbound connections hit the eBPF hallucinator, which swaps credentials
7. File access is scored by Shimmer (allow/alert/block)

## License

Apache 2.0. See [LICENSE](LICENSE).

This project is a derivative of [Kata Containers](https://github.com/kata-containers/kata-containers), which is also Apache 2.0 licensed.
