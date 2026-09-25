//! Port of packages/core/src/dedupe.ts. Every attachment cache entry becomes a
//! symlink to a file named by its SHA-1, so the renderer, which keys decoded
//! images on the path, decodes forty copies of one GIF once.

use std::path::{Component, Path, PathBuf};

use sha1::{Digest, Sha1};
use tokio::io::AsyncReadExt;

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Hex SHA-1 of a string, for names derived from guids.
pub(crate) fn sha1_hex(text: &str) -> String {
    hex(&Sha1::digest(text.as_bytes()))
}

/// Streamed hex SHA-1, so a sixty megabyte video never sits in memory.
pub async fn file_digest(path: &Path) -> std::io::Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

#[cfg(unix)]
async fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    tokio::fs::symlink(target, link).await
}

#[cfg(windows)]
async fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    tokio::fs::symlink_file(target, link).await
}

/// Moves `path` to `<sha1><extension>` beside it and leaves a relative symlink (else a hard link, else a copy).
/// Returns the shared path, or `path` itself where neither link works.
pub async fn share_by_content(path: &Path, extension: &str) -> std::io::Result<PathBuf> {
    let dir = path.parent().unwrap_or(Path::new(""));
    let name = format!("{}{extension}", file_digest(path).await?);
    let shared = dir.join(&name);
    if shared == path {
        return Ok(shared);
    }
    if tokio::fs::symlink_metadata(&shared).await.is_ok() {
        match tokio::fs::remove_file(path).await {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
    } else {
        tokio::fs::rename(path, &shared).await?;
    }
    if symlink(Path::new(&name), path).await.is_ok() || tokio::fs::hard_link(&shared, path).await.is_ok() {
        return Ok(shared);
    }
    tokio::fs::copy(&shared, path).await?;
    Ok(path.to_path_buf())
}

/// `path.resolve` without touching the filesystem: `.` and `..` folded lexically.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Blocking `cached_file`, for resolving a whole page of entries in one `spawn_blocking`.
pub fn cached_file_blocking(path: &Path) -> Option<PathBuf> {
    let info = std::fs::symlink_metadata(path).ok()?;
    if !info.file_type().is_symlink() {
        return Some(path.to_path_buf());
    }
    let target = normalize(&path.parent().unwrap_or(Path::new("")).join(std::fs::read_link(path).ok()?));
    std::fs::metadata(&target).ok()?;
    Some(target)
}

/// The shared file behind a symlink, the entry itself otherwise, None when missing or dangling.
/// Follows only the link, never the directories above it, so one file is always spelled one way.
pub async fn cached_file(path: &Path) -> Option<PathBuf> {
    let info = tokio::fs::symlink_metadata(path).await.ok()?;
    if !info.file_type().is_symlink() {
        return Some(path.to_path_buf());
    }
    let target = normalize(&path.parent().unwrap_or(Path::new("")).join(tokio::fs::read_link(path).await.ok()?));
    tokio::fs::metadata(&target).await.ok()?;
    Some(target)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fresh directory under the workspace target dir, removed on drop.
    pub(crate) struct TestDir(pub PathBuf);

    impl TestDir {
        pub(crate) fn new(label: &str) -> Self {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/test-tmp")
                .join(format!("{label}-{}-{}", std::process::id(), fastrand::u64(..)));
            std::fs::create_dir_all(&dir).unwrap();
            TestDir(normalize(&dir))
        }

        pub(crate) fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn two_entries_with_the_same_bytes_end_up_behind_one_shared_file() {
        let dir = TestDir::new("dedupe");
        let (a, b) = (dir.join("guid-a.gif"), dir.join("guid-b.gif"));
        std::fs::write(&a, "GIF89a same bytes").unwrap();
        std::fs::write(&b, "GIF89a same bytes").unwrap();
        let shared_a = share_by_content(&a, ".gif").await.unwrap();
        let shared_b = share_by_content(&b, ".gif").await.unwrap();
        assert_eq!(shared_a, shared_b);
        assert_eq!(shared_a, dir.join(&format!("{}.gif", file_digest(&shared_a).await.unwrap())));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "GIF89a same bytes");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "GIF89a same bytes");
    }

    #[tokio::test]
    async fn different_bytes_stay_apart() {
        let dir = TestDir::new("dedupe");
        let (a, b) = (dir.join("guid-a.png"), dir.join("guid-b.png"));
        std::fs::write(&a, "one").unwrap();
        std::fs::write(&b, "two").unwrap();
        assert_ne!(share_by_content(&a, ".png").await.unwrap(), share_by_content(&b, ".png").await.unwrap());
    }

    #[tokio::test]
    async fn resolves_a_shared_entry_to_its_file_and_a_plain_entry_to_itself() {
        let dir = TestDir::new("dedupe");
        let plain = dir.join("plain.jpg");
        std::fs::write(&plain, "jpeg").unwrap();
        assert_eq!(cached_file(&plain).await, Some(plain.clone()));
        let entry = dir.join("guid.jpg");
        std::fs::write(&entry, "jpeg").unwrap();
        let shared = share_by_content(&entry, ".jpg").await.unwrap();
        assert_eq!(cached_file(&entry).await, Some(shared.clone()));
        assert_eq!(cached_file_blocking(&entry), Some(shared));
    }

    #[tokio::test]
    async fn is_none_for_a_missing_entry_and_for_a_link_whose_target_is_gone() {
        let dir = TestDir::new("dedupe");
        assert_eq!(cached_file(&dir.join("missing.jpg")).await, None);
        let entry = dir.join("guid.jpg");
        std::fs::write(&entry, "jpeg").unwrap();
        let shared = share_by_content(&entry, ".jpg").await.unwrap();
        std::fs::remove_file(&shared).unwrap();
        assert_eq!(cached_file(&entry).await, None);
        assert_eq!(cached_file_blocking(&entry), None);
    }
}
