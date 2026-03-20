// Package net sets up eBPF network interception.
//
// Detects the VM's network interface (spr0 on Sprites, eth0 elsewhere).
// Attaches TC clsact qdisc so eBPF programs can be loaded.
// Creates BPF pin directory for credential routing maps.
//
// The actual eBPF programs are loaded by Shimmer from outside.
// We just set up the attachment points.
package net

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/rs/zerolog"
)

const bpfPinDir = "/sys/fs/bpf/aether"

// Setup detects the network interface and prepares eBPF attachment points.
func Setup(l *zerolog.Logger) {
	iface := detectInterface()
	if iface == "" {
		l.Warn().Msg("net: no network interface found")
		return
	}

	l.Info().Str("interface", iface).Msg("net: detected")

	// Create BPF pin directory
	if err := os.MkdirAll(bpfPinDir, 0o755); err != nil {
		l.Warn().Err(err).Msg("net: failed to create BPF pin dir")
	}

	// Attach clsact qdisc (idempotent)
	out, err := exec.Command("tc", "qdisc", "add", "dev", iface, "clsact").CombinedOutput()
	if err != nil {
		msg := strings.TrimSpace(string(out))
		if strings.Contains(msg, "File exists") {
			l.Info().Str("interface", iface).Msg("net: clsact already attached")
		} else {
			l.Warn().Str("output", msg).Msg("net: tc clsact failed")
		}
		return
	}

	l.Info().Str("interface", iface).Msg("net: clsact attached")
}

func detectInterface() string {
	for _, name := range []string{"spr0", "eth0"} {
		if _, err := os.Stat(filepath.Join("/sys/class/net", name)); err == nil {
			return name
		}
	}

	// Try any non-lo interface
	entries, err := os.ReadDir("/sys/class/net")
	if err != nil {
		return ""
	}

	for _, e := range entries {
		if e.Name() != "lo" {
			return e.Name()
		}
	}

	return ""
}
