# Æther Agent

A daemon that runs inside sandboxed VMs (Sprites, E2B) and exposes the filesystem and network as programmable gates. Shimmer — your control plane — pushes files in, configures network rules, and manages tools. Processes inside see a normal Linux environment.

Tested and running on [Sprites](https://sprites.dev) (Fly.io Firecracker VMs).

## How It Works

```
Outside the VM (Shimmer / your control plane)
    │
    ├─ ttrpc ──────────► aether-agent (daemon inside the VM)
    │                         │
    │                         ├── AgentFS: files appear when Shimmer puts them
    │                         ├── Network: eBPF on spr0, rules from Shimmer
    │                         └── context.json: agents inside know what tools exist
    │
    └─ eBPF TC on spr0 ──► all traffic intercepted at kernel level
```

The agent doesn't fetch files. It doesn't decide what tools to install. It doesn't manage credentials. Shimmer does all of that from outside. The agent is the gate — it accepts what Shimmer pushes and presents it to processes inside.

## Quick Start (Sprites)

```bash
# Install Sprites CLI
curl https://sprites.dev/install.sh | bash
sprite login

# Create a Sprite
sprite create -o your-org my-sandbox

# Install the agent
sprite exec -s my-sandbox -- sh -c '
  curl -fsSL -o /tmp/aether-agent \
    "https://github.com/Atro-Tech/aether-agent/raw/main/dist/linux-x86_64/aether-agent"
  sudo cp /tmp/aether-agent /usr/bin/aether-agent
  sudo chmod +x /usr/bin/aether-agent
  sudo mkdir -p /run/aether
  sudo mount -t bpf bpf /sys/fs/bpf
'

# Start the agent
sprite exec -s my-sandbox -- sudo sh -c '
  KATA_AGENT_SERVER_ADDR=unix:///run/aether/agent.sock \
  nohup /usr/bin/aether-agent > /tmp/aether.log 2>&1 &
'

# Checkpoint it (golden image — every restore boots with aether ready)
sprite checkpoint create -s my-sandbox
```

## Connect via ttrpc

```go
conn, _ := net.Dial("unix", "/run/aether/agent.sock")
client := ttrpc.NewClient(conn)

// Put a file — it appears in the filesystem immediately
client.Call(ctx, "aether.AetherAgent", "RegisterAddIn", req, resp)

// Configure network rules — eBPF enforces at kernel level
client.Call(ctx, "aether.AetherAgent", "UpdateProxyRules", rules, resp)
```

## API (ttrpc)

Service: `aether.AetherAgent`

| RPC | What it does |
|-----|-------------|
| `RegisterAddIn` | Push an add-in manifest + files into the VM |
| `MaterializePath` | Push a file at a specific path |
| `UpdateProxyRules` | Configure credential routing for eBPF network interception |
| `GetManifest` | List files the agent knows about |
| `StreamLogs` | Stream logs from inside |

Proto definition: [`protocols/protos/aether.proto`](protocols/protos/aether.proto)

## What the Agent Does at Boot

```
1. AgentFS ready (filesystem accepts file pushes)
2. Network interceptor: detect spr0, attach TC clsact qdisc
3. ttrpc server starts on unix:///run/aether/agent.sock
4. Supervisor starts (heartbeat, watchdog)
5. Ready — Shimmer can connect
```

## AgentFS (Layer 3)

FUSE IS the filesystem. Three layers, top wins:

```
┌─────────────────────────────┐
│ Writable   — writes from    │ ← pip install lands here
│              inside          │
├─────────────────────────────┤
│ Add-ins    — pushed by      │ ← Shimmer puts /usr/bin/gh
│              Shimmer         │   and it just appears
├─────────────────────────────┤
│ Base       — golden image   │ ← /bin/sh, /usr/bin/python3
└─────────────────────────────┘
```

Shimmer calls `put("/usr/bin/gh", bytes, 0o755)` → file appears.
Shimmer calls `remove("/usr/bin/gh")` → file disappears.
Processes inside see a normal Linux filesystem.

## Network (Layer 2)

eBPF TC programs attach to the VM's network interface (`spr0` on Sprites). All traffic passes through. Shimmer configures credential routing rules via ttrpc → BPF maps.

Confirmed working on Sprites:
- Kernel 6.12.47-fly with full BPF support
- `CAP_SYS_ADMIN` + `CAP_NET_ADMIN` available
- BPF map creation works
- TC clsact qdisc attachment works

## Context (for agents inside)

`/run/aether/context.json` tells the agent inside what tools it has:

```json
{
  "version": "1",
  "request_tools": {
    "ttrpc_socket": "unix:///run/aether/agent.sock",
    "request_file": "/run/aether/requests/tool_request.json"
  },
  "tools": [
    {
      "command": "gh",
      "path": "/usr/bin/gh",
      "addin": "gh-abc123",
      "tool_type": "hybrid",
      "capabilities": ["git", "pr", "issue"]
    }
  ]
}
```

The add-in defines what metadata to include. Æther passes it through.

## Project Structure

```
aether-agent/
├── agent/              # Daemon (forked Kata agent, Rust)
│   └── src/
│       ├── main.rs         # Boot sequence
│       ├── rpc.rs          # ttrpc handlers (Shimmer talks to these)
│       ├── aether_net.rs   # eBPF network interceptor setup
│       └── aether_watchdog.rs  # Supervisor
├── aether-fs/          # AgentFS — the filesystem (put/remove/read/write)
├── aether-service/     # Manifest parsing + context.json
├── protocols/          # aether.proto + generated ttrpc code
├── packages/           # Example add-in manifests
├── clients/            # Go + Elixir client stubs
├── spec/               # Interop spec for Sprites/E2B integration
└── dist/               # Pre-built Linux x86_64 binary (14MB, static musl)
```

## Tested On

| Platform | Status |
|----------|--------|
| Sprites (Fly.io) | Working — booted, ttrpc connected, RPCs responding |
| E2B | Architecture compatible (Firecracker + virtio-net) |

## License

Apache 2.0. Forked from [Kata Containers](https://github.com/kata-containers/kata-containers).
