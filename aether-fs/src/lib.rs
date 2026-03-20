// Æther Agent FUSE overlay.
//
// Mounts over directories like /usr/bin, /usr/lib, /opt.
// Snapshots the real contents first, then serves them through FUSE.
// New files can be added (eager or lazy). Writes go to a real
// ext4 directory via a bypass fd.
//
// From the outside (Shimmer):
//   - CopyFile to /usr/bin/gh → bytes land in writable layer via FUSE
//   - Register lazy entry → file appears instantly, bytes come on first read
//
// From the inside (processes):
//   - ls /usr/bin → sees base files + added files
//   - cat /usr/bin/gh → served from FUSE (lazy fetch if needed)
//   - pip install foo → writes go to writable layer
//
// If the agent dies, FUSE unmounts and the real directory reappears.

pub mod overlay;
