// Package fuse mounts a FUSE overlay at the namespace root.
//
// Uses go-fuse's LoopbackNode as the base — proxies all operations
// to the real filesystem underneath. On top of that, we can inject
// virtual files (lazy entries that appear instantly, bytes come later).
//
// The mount point is configurable. For Sprites, we mount at a staging
// path and pivot_root into it (or use bind mounts).
package fuse

import (
	"sync"
	"syscall"

	"github.com/hanwen/go-fuse/v2/fs"
	"github.com/hanwen/go-fuse/v2/fuse"
	"github.com/rs/zerolog"
	"golang.org/x/net/context"
)

// VirtualFile is a file that exists in the overlay but not on disk.
// Used for lazy loading: the file appears immediately, bytes come later.
type VirtualFile struct {
	Content []byte // nil = lazy (no bytes yet)
	Size    uint64 // reported to stat, even if Content is nil
	Mode    uint32 // file permissions
}

// Overlay is the root of the FUSE filesystem.
// It embeds LoopbackNode to proxy real files, and adds virtual files on top.
type Overlay struct {
	fs.LoopbackNode

	mu       sync.RWMutex
	virtuals map[string]*VirtualFile // name → virtual file (top level only for now)

	logger *zerolog.Logger
}

// Put adds a virtual file. It appears immediately in the filesystem.
func (o *Overlay) Put(name string, content []byte, mode uint32) {
	o.mu.Lock()
	defer o.mu.Unlock()

	o.virtuals[name] = &VirtualFile{
		Content: content,
		Size:    uint64(len(content)),
		Mode:    mode,
	}
}

// PutLazy adds a virtual file with no content yet.
// stat returns the declared size. read blocks until Fill is called.
func (o *Overlay) PutLazy(name string, size uint64, mode uint32) {
	o.mu.Lock()
	defer o.mu.Unlock()

	o.virtuals[name] = &VirtualFile{
		Content: nil,
		Size:    size,
		Mode:    mode,
	}
}

// Fill provides content for a lazy file.
func (o *Overlay) Fill(name string, content []byte) {
	o.mu.Lock()
	defer o.mu.Unlock()

	if vf, ok := o.virtuals[name]; ok {
		vf.Content = content
		vf.Size = uint64(len(content))
	}
}

// Remove removes a virtual file.
func (o *Overlay) Remove(name string) {
	o.mu.Lock()
	defer o.mu.Unlock()

	delete(o.virtuals, name)
}

// Lookup intercepts name resolution. Checks virtual files first,
// then falls through to the real filesystem.
var _ = (fs.NodeLookuper)((*Overlay)(nil))

func (o *Overlay) Lookup(ctx context.Context, name string, out *fuse.EntryOut) (*fs.Inode, syscall.Errno) {
	// Check virtual files first
	o.mu.RLock()
	vf, ok := o.virtuals[name]
	o.mu.RUnlock()

	if ok {
		stable := fs.StableAttr{Mode: syscall.S_IFREG}
		child := o.NewPersistentInode(ctx, &virtualNode{vf: vf}, stable)
		out.Attr.Mode = vf.Mode | syscall.S_IFREG
		out.Attr.Size = vf.Size
		return child, 0
	}

	// Fall through to real filesystem
	return o.LoopbackNode.Lookup(ctx, name, out)
}

// Readdir merges virtual entries with real directory entries.
var _ = (fs.NodeReaddirer)((*Overlay)(nil))

func (o *Overlay) Readdir(ctx context.Context) (fs.DirStream, syscall.Errno) {
	// Get real entries
	realStream, errno := o.LoopbackNode.Readdir(ctx)
	if errno != 0 {
		return nil, errno
	}

	// Read all real entries
	var entries []fuse.DirEntry
	for realStream.HasNext() {
		e, err := realStream.Next()
		if err != 0 {
			break
		}
		entries = append(entries, e)
	}

	// Add virtual entries (skip if name already exists in real)
	realNames := make(map[string]bool)
	for _, e := range entries {
		realNames[e.Name] = true
	}

	o.mu.RLock()
	for name := range o.virtuals {
		if !realNames[name] {
			entries = append(entries, fuse.DirEntry{
				Name: name,
				Mode: syscall.S_IFREG,
			})
		}
	}
	o.mu.RUnlock()

	return &sliceDirStream{entries: entries}, 0
}

// virtualNode serves a virtual file's content.
type virtualNode struct {
	fs.Inode
	vf *VirtualFile
}

var _ = (fs.NodeGetattrer)((*virtualNode)(nil))

func (n *virtualNode) Getattr(ctx context.Context, f fs.FileHandle, out *fuse.AttrOut) syscall.Errno {
	out.Attr.Mode = n.vf.Mode | syscall.S_IFREG
	out.Attr.Size = n.vf.Size
	return 0
}

var _ = (fs.NodeOpener)((*virtualNode)(nil))

func (n *virtualNode) Open(ctx context.Context, flags uint32) (fs.FileHandle, uint32, syscall.Errno) {
	if n.vf.Content == nil {
		// Lazy file — no content yet. Return EIO.
		// The caller (Shimmer) should Fill() and retry.
		return nil, 0, syscall.EIO
	}

	return &virtualFileHandle{data: n.vf.Content}, 0, 0
}

var _ = (fs.NodeReader)((*virtualNode)(nil))

func (n *virtualNode) Read(ctx context.Context, f fs.FileHandle, dest []byte, off int64) (fuse.ReadResult, syscall.Errno) {
	if n.vf.Content == nil {
		return nil, syscall.EIO
	}

	end := int(off) + len(dest)
	if end > len(n.vf.Content) {
		end = len(n.vf.Content)
	}
	if int(off) >= len(n.vf.Content) {
		return fuse.ReadResultData(nil), 0
	}

	return fuse.ReadResultData(n.vf.Content[off:end]), 0
}

// virtualFileHandle is a file handle for reading virtual file content.
type virtualFileHandle struct {
	data []byte
}

var _ = (fs.FileReader)((*virtualFileHandle)(nil))

func (fh *virtualFileHandle) Read(ctx context.Context, dest []byte, off int64) (fuse.ReadResult, syscall.Errno) {
	end := int(off) + len(dest)
	if end > len(fh.data) {
		end = len(fh.data)
	}
	if int(off) >= len(fh.data) {
		return fuse.ReadResultData(nil), 0
	}

	return fuse.ReadResultData(fh.data[off:end]), 0
}

// sliceDirStream serves directory entries from a slice.
type sliceDirStream struct {
	entries []fuse.DirEntry
	pos     int
}

func (s *sliceDirStream) HasNext() bool {
	return s.pos < len(s.entries)
}

func (s *sliceDirStream) Next() (fuse.DirEntry, syscall.Errno) {
	if s.pos >= len(s.entries) {
		return fuse.DirEntry{}, syscall.EIO
	}
	e := s.entries[s.pos]
	s.pos++
	return e, 0
}

func (s *sliceDirStream) Close() {}

// Mount mounts the FUSE overlay.
// basePath is the real filesystem root to proxy.
// mountPoint is where to mount FUSE.
func Mount(l *zerolog.Logger) {
	// TODO: wire this into the boot sequence.
	// For now, log readiness. The actual mount requires:
	// 1. Choose basePath and mountPoint
	// 2. Create LoopbackRoot pointing at basePath
	// 3. Create Overlay wrapping it
	// 4. fs.Mount(mountPoint, overlay, opts)
	// 5. Run server.Wait() in a goroutine
	l.Info().Msg("FUSE overlay ready (mount pending)")
}

// NewOverlay creates a FUSE overlay backed by a real directory.
func NewOverlay(basePath string, l *zerolog.Logger) (*Overlay, *fs.LoopbackRoot, error) {
	var st syscall.Stat_t
	if err := syscall.Stat(basePath, &st); err != nil {
		return nil, nil, err
	}

	root := &fs.LoopbackRoot{
		Path: basePath,
		Dev:  uint64(st.Dev),
	}

	overlay := &Overlay{
		LoopbackNode: fs.LoopbackNode{RootData: root},
		virtuals:     make(map[string]*VirtualFile),
		logger:       l,
	}

	root.RootNode = overlay

	return overlay, root, nil
}

// MountAt mounts the FUSE overlay at the given path.
// Returns a server that can be stopped, and a handle to the overlay
// for adding/removing virtual files.
func MountAt(basePath, mountPoint string, l *zerolog.Logger) (*fuse.Server, *Overlay, error) {
	overlay, _, err := NewOverlay(basePath, l)
	if err != nil {
		return nil, nil, err
	}

	opts := &fs.Options{
		MountOptions: fuse.MountOptions{
			AllowOther: true,
			FsName:     "aether",
			Name:       "aether",
		},
	}

	server, err := fs.Mount(mountPoint, overlay, opts)
	if err != nil {
		return nil, nil, err
	}

	l.Info().
		Str("base", basePath).
		Str("mount", mountPoint).
		Msg("FUSE overlay mounted")

	return server, overlay, nil
}
