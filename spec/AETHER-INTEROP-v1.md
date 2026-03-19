# Æther Agent Interop Spec v1

**Status:** Draft
**Target runtimes:** Sprites, E2B, Fly.io, Modal, any container/VM sandbox

This spec defines how external agent runtimes plug into the Æther Agent to get credential hallucination, effect loading, lazy materialization, and file I/O monitoring — without forking or vendoring Æther.

---

## 1. Connection

The Æther Agent exposes a single **ttrpc** endpoint.

| Mode | Address |
|------|---------|
| Namespace (default) | `unix:///run/aether/agent.sock` |
| Firecracker/microVM | `vsock://2:1024` (guest CID 2, port 1024) |
| E2B sandbox | `unix:///run/aether/agent.sock` (inside the sandbox) |
| Sprites | `unix:///run/aether/agent.sock` (inside the sprite) |

**Protocol:** ttrpc (not gRPC). Lightweight, no HTTP/2. Libraries exist for Go, Rust, C. For languages without ttrpc support, use the sidecar gateway (see Section 7).

---

## 2. Lifecycle

```
1. Runtime boots sandbox (namespace, VM, or container)
2. Æther Agent starts as PID 1 or a sidecar
3. Agent opens ttrpc socket
4. Control plane connects and registers add-ins
5. Agent writes manifests to /run/aether/manifests/
6. Container/task is created — shim-loader hook fires
7. Effects are active for the task's lifetime
8. Task exits — overlay artifacts extracted, cleanup
```

### 2.1 For Sprites

```
sprite_create() →
  aether_agent starts inside sprite →
  control plane calls RegisterAddIn() for each tool →
  sprite runs user code with effects active
```

### 2.2 For E2B

```
e2b_sandbox.create() →
  aether_agent included in sandbox template →
  on sandbox boot, agent starts →
  SDK calls RegisterAddIn() before executing code →
  code runs with credential hallucination active
```

---

## 3. Core RPCs

All RPCs use protobuf encoding over ttrpc framing.

### 3.1 RegisterAddIn

Register a tool/package with its effect manifest.

```
Request:
  name: string           — "gh", "rclone-dropbox", etc.
  version: string        — semver
  manifest_toml: bytes   — full TOML manifest (see Section 5)
  content_address: string — CAS hash for lazy materialization

Response:
  success: bool
  addin_id: string       — deterministic ID (name + version hash)
  error: string          — empty on success
```

**When to call:** Before creating the task/container that needs this tool. Can be called multiple times to register multiple tools.

### 3.2 MaterializePath

Trigger lazy fetch of a specific file from a registered add-in.

```
Request:
  addin_id: string
  path: string           — relative path within the package ("bin/gh")

Response:
  success: bool
  host_path: string      — where the file now exists on disk
  error: string
```

**When to call:** Normally never — AgentFS handles this automatically on first file access. Use this RPC only for pre-warming.

### 3.3 UpdateProxyRules

Push credential routing rules to the eBPF hallucinator.

```
Request:
  rules: []ProxyRule
    match_pattern: string    — hostname glob ("*.dropbox.com")
    credential_key: string   — key name in the credential store
    target_address: string   — optional rewrite target

Response: Empty
```

**When to call:** After RegisterAddIn, before the task runs. The control plane must also populate the actual credential values separately (via a secure channel, not this RPC).

### 3.4 GetManifest

Retrieve the parsed manifest for a registered add-in.

```
Request:
  addin_id: string

Response:
  addin_id: string
  name: string
  version: string
  effects: []EffectEntry
    binary_name: string
    shim_library: string
    env: map<string,string>
  binaries: []string
  libraries: []string
```

### 3.5 StreamLogs

Stream real-time logs from add-in execution.

```
Request:
  addin_id: string      — filter by add-in, or empty for all
  level: string         — minimum level: "debug", "info", "warn", "error"

Response (stream):
  timestamp: string
  level: string
  message: string
  addin_id: string
```

---

## 4. Filesystem Layout

Runtimes MUST provide these paths inside the sandbox:

```
/run/aether/
  agent.sock              — ttrpc socket
  manifests/              — registered manifest TOML files
    {addin_id}.toml
  env/                    — shim-loader output
    ld_preload            — colon-separated LD_PRELOAD paths
    extra_env             — KEY=VALUE per line
  ebpf-staging/           — credential routing data
    credentials/
    pids/
    shimmer/

/usr/libexec/aether/
  shim-loader             — the OCI StartContainer hook binary

/usr/lib/aether/
  shims/                  — LD_PRELOAD .so files
    lib{name}_shim.so
  {package_name}/         — materialized package files

/var/cache/aether/        — AgentFS materialization cache
  {content_address}/
    {relative_path}
```

---

## 5. Manifest Format (TOML)

Every add-in is defined by a TOML manifest. This is the contract between package authors and the Æther runtime.

```toml
[package]
name = "string"               # required
version = "string"             # required, semver
description = "string"         # optional
content_address = "string"     # CAS hash for lazy materialization

[[effects]]                    # one or more effects per package
binary_name = "string"         # command that triggers this effect
shim_library = "string"        # path to LD_PRELOAD .so
[effects.env]                  # extra env vars
KEY = "value"

[[binaries]]                   # files to materialize
path = "string"                # relative path within package
source = "lazy"                # "lazy" (AgentFS) or "eager" (on register)

[[libraries]]                  # shared libraries
path = "string"
source = "lazy"

[proxy_rules]                  # credential routing
[[proxy_rules.rules]]
match_pattern = "string"       # hostname glob
credential_key = "string"      # key in eBPF credential map
target_address = "string"      # optional proxy rewrite
```

---

## 6. Integration Patterns

### 6.1 Minimal Integration (any runtime)

Just start the agent binary and connect over the unix socket. No kernel modifications needed. Effects work via LD_PRELOAD only (no eBPF).

```
1. Include `aether-agent` binary in your sandbox image
2. Start it: `aether-agent --server-addr unix:///run/aether/agent.sock`
3. Connect via ttrpc from your control plane
4. Call RegisterAddIn() with manifests
5. Launch tasks — effects are automatically applied
```

### 6.2 Full Integration (with eBPF)

Requires Linux 5.8+ with BPF enabled. Adds credential hallucination at the kernel level.

```
1. Same as minimal, plus:
2. Load the hallucinator eBPF probe on sandbox boot
3. Call UpdateProxyRules() to configure credential routing
4. Populate actual credentials via your secure channel
```

### 6.3 E2B Integration

```python
from e2b import Sandbox

sandbox = Sandbox(template="aether-enabled")

# Register tools before running code
sandbox.rpc("RegisterAddIn", {
    "name": "gh",
    "version": "2.62.0",
    "manifest_toml": open("packages/gh/manifest.toml", "rb").read(),
})

# Run code — gh commands automatically get credential injection
result = sandbox.run("gh repo list")
```

### 6.4 Sprites Integration

```javascript
const sprite = await Sprites.create({
  template: "aether",
  addins: ["gh", "rclone-dropbox"],  // pre-registered
});

// Effects are already active
const result = await sprite.exec("gh pr list --repo owner/repo");
```

---

## 7. Sidecar Gateway (for non-ttrpc languages)

For runtimes that can't speak ttrpc natively, Æther provides a thin HTTP gateway:

```
POST /v1/addins              → RegisterAddIn
POST /v1/materialize         → MaterializePath
PUT  /v1/proxy-rules         → UpdateProxyRules
GET  /v1/addins/{id}         → GetManifest
GET  /v1/logs?addin={id}     → StreamLogs (SSE)
```

The gateway binary (`aether-gateway`) connects to the agent's unix socket and exposes these endpoints on `localhost:1025`.

---

## 8. Health & Watchdog

The agent includes a built-in watchdog. Runtimes can monitor health via:

**ttrpc Health.Check** (existing Kata endpoint):
```
Request: {} → Response: { status: SERVING }
```

**Watchdog log signals** (monitor stderr/syslog):
```
"Æther watchdog: N consecutive failures" → agent is degraded
"Æther watchdog recovered"               → agent is healthy again
```

**Hardware watchdog** (`/dev/watchdog`): Agent pets it every 10s. If agent hangs, VM reboots. Enable by creating the device in your sandbox.

---

## 9. Security Model

| Layer | What it protects |
|-------|-----------------|
| LD_PRELOAD shims | Intercept library calls, inject env vars |
| eBPF hallucinator | Swap credentials at syscall level — process never sees real secrets |
| Shimmer | Monitor file I/O, block suspicious patterns |
| Namespace isolation | Process can't escape its sandbox |
| Manifest signing | (planned) Verify manifest integrity before loading |

**Credentials never enter the guest.** The eBPF probe reads from kernel-space pinned maps that are populated by the host-side control plane. The guest process only ever sees placeholder tokens.

---

## 10. Versioning

This spec is versioned independently from the agent binary.

| Spec version | Agent version | Breaking changes |
|-------------|--------------|-----------------|
| v1 (this) | 0.1.x | Initial release |

Runtimes SHOULD include `Aether-Spec-Version: v1` in their integration metadata so the agent can validate compatibility.
