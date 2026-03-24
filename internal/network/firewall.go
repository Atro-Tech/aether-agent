// Package network manages iptables-based egress firewall.
//
// Default-deny on boot. Allows loopback, DNS, and the tunnel endpoint.
// Manifest updates add ACCEPT rules for allowed network destinations.
package network

import (
	"fmt"
	"net"
	"os/exec"
	"strings"

	"github.com/atro-tech/aether-agent/internal/manifest"
	"github.com/rs/zerolog"
)

const chainName = "AETHER_EGRESS"

// Firewall manages the iptables egress chain.
type Firewall struct {
	logger     *zerolog.Logger
	tunnelHost string
	tunnelPort int
}

// Setup creates the AETHER_EGRESS chain with default-deny.
// tunnelHost/tunnelPort are exempted so the agent can reach the backend.
func Setup(logger *zerolog.Logger, tunnelHost string, tunnelPort int) (*Firewall, error) {
	fw := &Firewall{
		logger:     logger,
		tunnelHost: tunnelHost,
		tunnelPort: tunnelPort,
	}

	if err := fw.init(); err != nil {
		return nil, err
	}

	return fw, nil
}

// Apply flushes the chain and rebuilds it from the given network entries.
func (fw *Firewall) Apply(entries []manifest.NetworkEntry) error {
	// Flush existing rules
	if err := iptables("-F", chainName); err != nil {
		return fmt.Errorf("flush chain: %w", err)
	}

	// Re-add base exemptions
	if err := fw.addBaseRules(); err != nil {
		return err
	}

	// Add rules for each allowed entry
	for _, entry := range entries {
		if err := fw.addEntryRules(entry); err != nil {
			fw.logger.Warn().Err(err).Str("pattern", entry.Pattern).Msg("failed to add firewall rule")
		}
	}

	// Final DROP
	if err := iptables("-A", chainName, "-j", "DROP"); err != nil {
		return fmt.Errorf("add drop rule: %w", err)
	}

	fw.logger.Info().Int("rules", len(entries)).Msg("firewall rules applied")
	return nil
}

// Teardown removes the chain and the OUTPUT jump.
func (fw *Firewall) Teardown() error {
	_ = iptables("-D", "OUTPUT", "-j", chainName)
	_ = iptables("-F", chainName)
	_ = iptables("-X", chainName)
	return nil
}

// init creates the chain with default-deny.
func (fw *Firewall) init() error {
	// Create chain (ignore "already exists")
	if err := iptables("-N", chainName); err != nil {
		if !strings.Contains(err.Error(), "already exists") {
			return fmt.Errorf("create chain: %w", err)
		}
		// Chain exists — flush it
		if err := iptables("-F", chainName); err != nil {
			return fmt.Errorf("flush existing chain: %w", err)
		}
	}

	// Add jump from OUTPUT (idempotent — check first)
	if err := iptables("-C", "OUTPUT", "-j", chainName); err != nil {
		if err := iptables("-A", "OUTPUT", "-j", chainName); err != nil {
			return fmt.Errorf("add output jump: %w", err)
		}
	}

	// Add base rules
	if err := fw.addBaseRules(); err != nil {
		return err
	}

	// Default DROP
	if err := iptables("-A", chainName, "-j", "DROP"); err != nil {
		return fmt.Errorf("add drop rule: %w", err)
	}

	fw.logger.Info().Msg("firewall: default-deny egress installed")
	return nil
}

// addBaseRules adds loopback, tunnel, and DNS exemptions.
func (fw *Firewall) addBaseRules() error {
	// Loopback
	if err := iptables("-A", chainName, "-o", "lo", "-j", "ACCEPT"); err != nil {
		return fmt.Errorf("add loopback rule: %w", err)
	}

	// Tunnel endpoint
	if fw.tunnelHost != "" {
		ips := resolveHost(fw.tunnelHost)
		for _, ip := range ips {
			if err := iptables("-A", chainName, "-d", ip+"/32", "-p", "tcp", "--dport", fmt.Sprintf("%d", fw.tunnelPort), "-j", "ACCEPT"); err != nil {
				fw.logger.Warn().Err(err).Str("ip", ip).Msg("failed to add tunnel rule")
			}
		}
	}

	// DNS (UDP + TCP)
	if err := iptables("-A", chainName, "-p", "udp", "--dport", "53", "-j", "ACCEPT"); err != nil {
		return fmt.Errorf("add dns udp rule: %w", err)
	}
	if err := iptables("-A", chainName, "-p", "tcp", "--dport", "53", "-j", "ACCEPT"); err != nil {
		return fmt.Errorf("add dns tcp rule: %w", err)
	}

	return nil
}

// addEntryRules resolves a manifest network entry to iptables ACCEPT rules.
func (fw *Firewall) addEntryRules(entry manifest.NetworkEntry) error {
	host, portStr := splitHostPort(entry.Pattern)
	if host == "" {
		return fmt.Errorf("invalid pattern: %s", entry.Pattern)
	}

	ips := resolveHost(host)
	if len(ips) == 0 {
		fw.logger.Warn().Str("host", host).Msg("firewall: DNS resolve returned no IPs")
		// Still add a hostname-based rule as fallback
		args := []string{"-A", chainName, "-d", host, "-p", "tcp"}
		if portStr != "" {
			args = append(args, "--dport", portStr)
		}
		args = append(args, "-j", "ACCEPT")
		return iptables(args...)
	}

	for _, ip := range ips {
		args := []string{"-A", chainName, "-d", ip + "/32", "-p", "tcp"}
		if portStr != "" {
			args = append(args, "--dport", portStr)
		}
		args = append(args, "-j", "ACCEPT")
		if err := iptables(args...); err != nil {
			return err
		}
	}

	return nil
}

// splitHostPort splits "host:port" into host and port parts.
func splitHostPort(pattern string) (string, string) {
	host, port, err := net.SplitHostPort(pattern)
	if err != nil {
		// No port — treat entire pattern as host
		return pattern, ""
	}
	return host, port
}

// resolveHost does a DNS lookup, returning IP strings.
func resolveHost(host string) []string {
	// If it's already an IP, return it
	if ip := net.ParseIP(host); ip != nil {
		return []string{host}
	}

	addrs, err := net.LookupHost(host)
	if err != nil {
		return nil
	}
	return addrs
}

// iptables runs an iptables command.
func iptables(args ...string) error {
	out, err := exec.Command("iptables", args...).CombinedOutput()
	if err != nil {
		return fmt.Errorf("iptables %s: %s (%w)", strings.Join(args, " "), strings.TrimSpace(string(out)), err)
	}
	return nil
}
