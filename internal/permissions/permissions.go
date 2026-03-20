// Package permissions provides user lookup and path resolution.
// Stripped-down version — no E2B auth, no token verification.
package permissions

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"os/user"
	"path/filepath"
	"strconv"
	"time"
)

// GetUser looks up a system user by username.
func GetUser(username string) (*user.User, error) {
	return user.Lookup(username)
}

// GetAuthUser returns the user for the current request.
func GetAuthUser(ctx context.Context, defaultUsername string) (*user.User, error) {
	return GetUser(defaultUsername)
}

// GetUserIdInts returns uid and gid as ints.
func GetUserIdInts(u *user.User) (int, int, error) {
	uid, err := strconv.Atoi(u.Uid)
	if err != nil {
		return 0, 0, fmt.Errorf("invalid uid: %w", err)
	}

	gid, err := strconv.Atoi(u.Gid)
	if err != nil {
		return 0, 0, fmt.Errorf("invalid gid: %w", err)
	}

	return uid, gid, nil
}

// GetUserIdUints returns uid and gid as uint32s.
func GetUserIdUints(u *user.User) (uint32, uint32, error) {
	uid, gid, err := GetUserIdInts(u)
	if err != nil {
		return 0, 0, err
	}

	return uint32(uid), uint32(gid), nil
}

// ExpandAndResolve expands ~ and resolves symlinks.
// workdir is used as fallback if path is relative.
func ExpandAndResolve(path string, u *user.User, workdir *string) (string, error) {
	if len(path) > 0 && path[0] == '~' {
		path = filepath.Join(u.HomeDir, path[1:])
	}

	if !filepath.IsAbs(path) && workdir != nil {
		path = filepath.Join(*workdir, path)
	}

	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return filepath.Clean(path), nil
	}

	return resolved, nil
}

// EnsureDirs creates parent directories with the given ownership.
func EnsureDirs(path string, uid, gid int) error {
	dir := filepath.Dir(path)

	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}

	return os.Chown(dir, uid, gid)
}

// GetKeepAliveTicker returns a ticker for keepalive messages.
func GetKeepAliveTicker(req interface{ Header() http.Header }) (*time.Ticker, func()) {
	interval := 30 * time.Second
	ticker := time.NewTicker(interval)

	reset := func() {
		ticker.Reset(interval)
	}

	return ticker, reset
}
