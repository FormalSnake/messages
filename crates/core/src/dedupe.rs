//! Port of packages/core/src/dedupe.ts. Every attachment cache entry becomes a
//! symlink to a file named by its SHA-1, so the renderer, which keys decoded
//! images on the path, decodes forty copies of one GIF once.

use std::path::{Path, PathBuf};

/// Streamed hex SHA-1, so a sixty megabyte video never sits in memory.
pub async fn file_digest(path: &Path) -> std::io::Result<String> {
    let _ = path;
    unimplemented!()
}

/// Moves `path` to `<sha1><extension>` beside it and leaves a relative symlink (else a hard link, else a copy).
pub async fn share_by_content(path: &Path, extension: &str) -> std::io::Result<PathBuf> {
    let _ = (path, extension);
    unimplemented!()
}

/// The shared file behind a symlink, the entry itself otherwise, None when missing or dangling.
/// Follows only the link, never the directories above it, so one file is always spelled one way.
pub async fn cached_file(path: &Path) -> Option<PathBuf> {
    let _ = path;
    unimplemented!()
}
