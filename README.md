# Æther Agent

Fork of [E2B envd](https://github.com/e2b-dev/infra/tree/main/packages/envd) with FUSE root filesystem and eBPF network interception. Runs inside Sprites (Fly.io) and E2B sandboxed VMs.

Same protocol as envd: ConnectRPC (protobuf over HTTP). Process exec with streaming stdio, file operations, filesystem watching.

## What Changed from envd

- **`main.go`** — Stripped E2B auth, MMDS, port forwarding. Added FUSE mount + eBPF net setup in boot sequence.
- **`internal/fuse/`** — FUSE filesystem at namespace root `/`. (Implementation pending.)
- **`internal/net/`** — eBPF TC setup on `spr0`/`eth0`. Confirmed working on Sprites.
- **`internal/permissions/`** — Simplified. No E2B token auth.
- **`internal/logs/`** — Simplified. No MMDS log forwarding.

Everything else (process service, filesystem service, handler, multiplexer) is stock envd.

## Boot Sequence

```
1. Mount FUSE at /          (the filesystem)
2. Attach eBPF TC on spr0   (network interception)
3. Start ConnectRPC server   (Shimmer connects here)
```

## API

Process service (ConnectRPC):
- `Start` — server stream: StartEvent -> DataEvent* -> EndEvent
- `Connect` — attach to running process
- `StreamInput` / `SendInput` — write to stdin
- `SendSignal` — SIGTERM, SIGKILL
- `Update` — PTY resize
- `List` — all tracked processes

Filesystem service (ConnectRPC):
- `Stat`, `MakeDir`, `Move`, `ListDir`, `Remove`
- `WatchDir` — streaming filesystem events

File upload/download: `POST /files`, `GET /files`

## Quick Start (Sprites)

```bash
sprite exec -s my-sandbox -- sh -c '
  curl -fsSL -o /tmp/aether-agent \
    "https://github.com/Atro-Tech/aether-agent/raw/main/dist/linux-x86_64/aether-agent"
  sudo cp /tmp/aether-agent /usr/bin/aether-agent
  sudo chmod +x /usr/bin/aether-agent
  sudo mount -t bpf bpf /sys/fs/bpf
  sudo /usr/bin/aether-agent &
'
```

## License

Apache 2.0. Forked from [E2B](https://github.com/e2b-dev/infra) (Apache 2.0).
