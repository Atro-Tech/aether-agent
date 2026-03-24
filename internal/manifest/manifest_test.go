package manifest

import (
	"testing"
)

func TestMatchGlob(t *testing.T) {
	tests := []struct {
		pattern string
		path    string
		want    bool
	}{
		// Exact match
		{"/usr/local/bin/claude", "/usr/local/bin/claude", true},
		{"/usr/local/bin/claude", "/usr/local/bin/node", false},

		// Trailing /**
		{"/usr/**", "/usr/bin", true},
		{"/usr/**", "/usr/local/bin/claude", true},
		{"/usr/**", "/usr", true},
		{"/usr/**", "/etc/passwd", false},
		{"/tmp/**", "/tmp/foo/bar", true},

		// Trailing /*
		{"/usr/*", "/usr/bin", true},
		{"/usr/*", "/usr/local/bin", false}, // only one level

		// Simple filepath.Match
		{"/etc/*.conf", "/etc/nginx.conf", true},
		{"/etc/*.conf", "/etc/nginx/main.conf", false},
	}

	for _, tt := range tests {
		got := matchGlob(tt.pattern, tt.path)
		if got != tt.want {
			t.Errorf("matchGlob(%q, %q) = %v, want %v", tt.pattern, tt.path, got, tt.want)
		}
	}
}

func TestStoreNetworkAllowed(t *testing.T) {
	s := NewStore()

	// No manifest — everything denied
	if s.IsNetworkAllowed("api.anthropic.com", 443) {
		t.Error("expected deny with no manifest")
	}

	s.Update(&Manifest{
		Network: []NetworkEntry{
			{Pattern: "api.anthropic.com:443", Protocol: "https"},
		},
	})

	if !s.IsNetworkAllowed("api.anthropic.com", 443) {
		t.Error("expected allow for api.anthropic.com:443")
	}
	if s.IsNetworkAllowed("evil.com", 443) {
		t.Error("expected deny for evil.com:443")
	}
}

func TestStorePathAllowed(t *testing.T) {
	s := NewStore()

	// No manifest — bootstrap grace, everything allowed
	allowed, _ := s.IsPathAllowed("/anything")
	if !allowed {
		t.Error("expected allow during bootstrap grace")
	}

	s.Update(&Manifest{
		Filesystem: []FilesystemEntry{
			{Path: "/usr/**", Mode: "read_only", Source: "passthrough"},
			{Path: "/tmp/**", Mode: "read_write", Source: "passthrough"},
			{Path: "/etc/resolv.conf", Mode: "read_only", Source: "passthrough"},
		},
	})

	tests := []struct {
		path string
		want bool
	}{
		{"/usr/bin/bash", true},
		{"/usr/local/bin/claude", true},
		{"/tmp/test.txt", true},
		{"/etc/resolv.conf", true},
		{"/etc/shadow", false},
		{"/root/.ssh/id_rsa", false},
		{"/var/log/syslog", false},
	}

	for _, tt := range tests {
		got, _ := s.IsPathAllowed(tt.path)
		if got != tt.want {
			t.Errorf("IsPathAllowed(%q) = %v, want %v", tt.path, got, tt.want)
		}
	}
}

func TestStoreCallbacks(t *testing.T) {
	s := NewStore()
	called := false
	s.OnUpdate(func(m *Manifest) {
		called = true
	})

	s.Update(&Manifest{Version: 1})

	if !called {
		t.Error("callback was not called")
	}
}
