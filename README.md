# Æther Agent

A Kata agent fork that adds FUSE overlays and eBPF network interception for sandboxed AI agents. Runs inside Sprites (Fly.io) and E2B VMs.

Same Kata primitives: `ExecProcess`, `CopyFile`, `ReadStdout`, `WriteStdin`. No new RPC service. The agent just boots differently — mounts FUSE overlays on `/usr/bin`, `/usr/lib`, `/opt` for lazy file loading, and attaches eBPF TC on the network interface for traffic interception.

## What Changed from Kata

4 files modified:

- **`main.rs`** — Branding (`aether-agent`). Boot sequence calls `aether_net::setup()` to detect the network interface and attach TC clsact.
- **`Cargo.toml`** — Added `aether-fs` dependency. Fixed workspace paths.
- **`version.rs`** — Version info.
- **`aether_net.rs`** — New module. Detects `spr0` (Sprites) or `eth0` (E2B). Attaches TC clsact qdisc for eBPF program loading. Creates BPF pin directory.

Everything else is stock Kata agent.

## FUSE Overlays (aether-fs)

Mounts over specific directories. Snapshots the real contents first, opens a bypass fd to the underlying ext4, then serves files through FUSE.

```
/usr/bin/  ← FUSE overlay (base snapshot + lazy add-in files)
/usr/lib/  ← FUSE overlay
/opt/      ← FUSE overlay
/sbin/     ← real ext4 (agent lives here, never FUSE)
/etc/      ← real ext4
/tmp/      ← real ext4
```

Shimmer uses Kata's `CopyFile` to inject files. They go through FUSE to the writable layer. Lazy files appear instantly (metadata only) — bytes come on first read.

## Network Interception

eBPF TC programs attach to the VM's network interface. All traffic passes through. Shimmer loads the programs and configures credential routing from outside.

Confirmed on Sprites: kernel 6.12.47-fly, `CAP_SYS_ADMIN` + `CAP_NET_ADMIN`, BPF syscall works, TC clsact works.

## Quick Start (Sprites)

```bash
sprite create -o your-org my-sandbox

sprite exec -s my-sandbox -- sh -c '
  curl -fsSL -o /tmp/aether-agent \
    "https://github.com/Atro-Tech/aether-agent/raw/main/dist/linux-x86_64/aether-agent"
  sudo cp /tmp/aether-agent /usr/bin/aether-agent
  sudo chmod +x /usr/bin/aether-agent
'

sprite exec -s my-sandbox -- sudo sh -c '
  KATA_AGENT_SERVER_ADDR=unix:///run/aether/agent.sock \
  nohup /usr/bin/aether-agent > /tmp/aether.log 2>&1 &
'

# Checkpoint for instant boot
sprite checkpoint create -s my-sandbox
```

## Project Structure

```
agent/                 # The Kata agent (4 files modified)
  src/
    main.rs            # Boot: FUSE overlays + eBPF net setup
    aether_net.rs      # Network interceptor (detect iface, attach TC)
    version.rs         # Version info
    rpc.rs             # Stock Kata (ExecProcess, CopyFile, etc.)
    sandbox.rs         # Stock Kata
    ...                # Stock Kata (everything else unchanged)
aether-fs/             # FUSE overlay library
  src/overlay.rs       # Snapshot, put, lazy, fill, remove, bypass fd
protocols/             # Stock Kata protos (agent.proto, health.proto)
dist/                  # Pre-built Linux x86_64 binary
```

## License

Apache 2.0. Forked from [Kata Containers](https://github.com/kata-containers/kata-containers).
