# E7 Filesystem Abstraction Evidence

Date: 2026-04-01

## Scope Implemented

The kernel now includes a minimal VFS layer in `kernel/src/fs.rs` with:

- Mount type: `Mount { name, root }`
- Node type: `Node { id, name, parent, kind, data }`
- File handle type: `FileHandle { node, offset }`
- Error type: `VfsError`
- Root mount operation: `mount_root()`
- Path lookup subset: absolute paths with direct segment traversal
- File operations: `open()` and `read()`

## Root Filesystem Model

The initial root filesystem is an in-kernel static table:

- `/` (directory)
- `/etc` (directory)
- `/etc/motd` (file, contents: `kernel vfs motd\n`)
- `/hello.txt` (file, contents: `hello from rootfs\n`)

## Demonstrated Behavior

Boot probe `probe_vfs()` validates:

1. Root mount succeeds.
2. Lookup works for `/`, `/etc`, and `/etc/motd`.
3. Lookup for missing path returns `VfsError::NotFound`.
4. `open("/etc/motd")` returns a valid file handle.
5. `read()` returns expected bytes and content via the handle.

Expected serial lines:

- `fs: mount=1 root=1 etc=1 motd=1 miss=1 read_ok=1 read_bytes=16`
- `fs: vfs PASS`

## Known Limitations

- Read-only static filesystem; no write/create/remove.
- No mount namespace beyond a single root mount.
- No relative path support (`.`/`..` not implemented).
- No directories listing API.
- No per-process file descriptor table yet.
- No permissions, ownership, or timestamps.
