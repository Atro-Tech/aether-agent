// Package manifest stores the active sandbox manifest and provides rule matching.
//
// The manifest is the single source of truth for what network endpoints
// and filesystem paths are allowed. Updated via HTTP from the backend.
// Modules register callbacks to react to manifest changes.
package manifest

import (
	"path/filepath"
	"strconv"
	"strings"
	"sync"
)

// Manifest is the full sandbox policy pushed from the backend.
type Manifest struct {
	Version     int               `json:"version"`
	WorkspaceID string            `json:"workspace_id"`
	Network     []NetworkEntry    `json:"network"`
	Filesystem  []FilesystemEntry `json:"filesystem"`
	Credentials map[string]string `json:"credentials"`
}

// NetworkEntry is a single allowed network destination.
type NetworkEntry struct {
	AbilityID     string `json:"ability_id"`
	Pattern       string `json:"pattern"`        // "api.anthropic.com:443"
	Protocol      string `json:"protocol"`       // "https"
	Direction     string `json:"direction"`      // "outbound"
	CredentialKey string `json:"credential_key"` // optional
	Inspect       bool   `json:"inspect"`
}

// FilesystemEntry is a single allowed filesystem path pattern.
type FilesystemEntry struct {
	AbilityID string `json:"ability_id"`
	Path      string `json:"path"`      // glob: "/usr/**", "/usr/local/bin/claude"
	Direction string `json:"direction"` // "outside_in", "inside_out", "bidirectional"
	Mode      string `json:"mode"`      // "read_only", "read_write"
	Source    string `json:"source"`    // "passthrough", "snapshot", "dynamic"
	Persist   bool   `json:"persist"`
}

// Store holds the current manifest and notifies callbacks on update.
type Store struct {
	mu        sync.RWMutex
	current   *Manifest
	callbacks []func(*Manifest)

	// Pre-compiled for fast matching
	netIndex map[string]bool // "host:port" → allowed
	fsGlobs  []compiledGlob
}

type compiledGlob struct {
	pattern string
	entry   *FilesystemEntry
}

// NewStore creates an empty manifest store.
func NewStore() *Store {
	return &Store{
		netIndex: make(map[string]bool),
	}
}

// Update replaces the current manifest and fires callbacks.
func (s *Store) Update(m *Manifest) error {
	s.mu.Lock()
	s.current = m
	s.rebuildIndexLocked()
	cbs := make([]func(*Manifest), len(s.callbacks))
	copy(cbs, s.callbacks)
	s.mu.Unlock()

	for _, fn := range cbs {
		fn(m)
	}
	return nil
}

// Current returns the active manifest, or nil if none has been received.
func (s *Store) Current() *Manifest {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.current
}

// HasManifest returns true if any manifest has been received.
func (s *Store) HasManifest() bool {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.current != nil
}

// OnUpdate registers a callback fired after each manifest update.
func (s *Store) OnUpdate(fn func(*Manifest)) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.callbacks = append(s.callbacks, fn)
}

// IsNetworkAllowed checks if a host:port is allowed by the manifest.
func (s *Store) IsNetworkAllowed(host string, port int) bool {
	s.mu.RLock()
	defer s.mu.RUnlock()

	if s.current == nil {
		return false
	}

	key := host + ":" + strconv.Itoa(port)
	return s.netIndex[key]
}

// IsPathAllowed checks if a filesystem path is allowed by the manifest.
// Returns the matching entry if allowed. If no manifest exists, returns allowed=true (bootstrap grace).
func (s *Store) IsPathAllowed(path string) (bool, *FilesystemEntry) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	if s.current == nil {
		return true, nil
	}

	for _, g := range s.fsGlobs {
		if matchGlob(g.pattern, path) {
			return true, g.entry
		}
	}

	return false, nil
}

// rebuildIndexLocked rebuilds the fast-lookup indexes. Caller must hold write lock.
func (s *Store) rebuildIndexLocked() {
	m := s.current
	if m == nil {
		return
	}

	// Network: build host:port set
	s.netIndex = make(map[string]bool, len(m.Network))
	for i := range m.Network {
		pattern := m.Network[i].Pattern
		// Pattern is already "host:port" format
		s.netIndex[pattern] = true
	}

	// Filesystem: compile glob patterns
	s.fsGlobs = make([]compiledGlob, len(m.Filesystem))
	for i := range m.Filesystem {
		s.fsGlobs[i] = compiledGlob{
			pattern: m.Filesystem[i].Path,
			entry:   &m.Filesystem[i],
		}
	}
}

// matchGlob matches a path against a manifest glob pattern.
// Supports ** (any depth), * (single segment), and exact matches.
func matchGlob(pattern, path string) bool {
	// Exact match
	if pattern == path {
		return true
	}

	// Handle trailing /** — matches the prefix and everything under it
	if strings.HasSuffix(pattern, "/**") {
		prefix := strings.TrimSuffix(pattern, "/**")
		if path == prefix || strings.HasPrefix(path, prefix+"/") {
			return true
		}
		return false
	}

	// Handle trailing /* — matches one level of children
	if strings.HasSuffix(pattern, "/*") {
		prefix := strings.TrimSuffix(pattern, "/*")
		if !strings.HasPrefix(path, prefix+"/") {
			return false
		}
		rest := path[len(prefix)+1:]
		return !strings.Contains(rest, "/")
	}

	// Fall back to filepath.Match for simple globs
	matched, err := filepath.Match(pattern, path)
	if err != nil {
		return false
	}
	return matched
}
