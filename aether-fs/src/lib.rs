// AgentFS — the root filesystem for machines running in Æther.
//
// FUSE IS the filesystem. Three layers, top wins:
//
//   Writable  — writes from inside the machine land here
//   Add-ins   — files pushed in by Shimmer from outside
//   Base      — the golden image directory
//
// Shimmer puts files in. Processes inside read them. That's it.

pub mod fs;
