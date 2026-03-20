// Package fuse mounts a FUSE filesystem at the namespace root (/).
//
// Before mount: snapshots the real filesystem.
// After mount: all file operations go through our handler.
// The handler serves the original files plus anything Shimmer pushes in.
// If the agent dies, the mount drops and the real filesystem reappears.
package fuse

import (
	"github.com/rs/zerolog"
)

// Mount sets up the FUSE filesystem at /.
// Snapshots the real root, then mounts FUSE over it.
//
// TODO: implement with bazil.org/fuse or hanwen/go-fuse.
// For now, this is a placeholder that logs readiness.
func Mount(l *zerolog.Logger) {
	l.Info().Msg("FUSE: ready (mount pending implementation)")

	// The implementation will:
	// 1. Open bypass fd to real / before mounting
	// 2. Snapshot directory listings into memory
	// 3. Mount FUSE at /
	// 4. Serve reads from: snapshot + lazy entries + writable layer
	// 5. Route writes to writable layer on real ext4 via bypass fd
}
