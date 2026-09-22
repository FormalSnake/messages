import { createHash } from 'node:crypto'
import { createReadStream } from 'node:fs'
import { copyFile, link, lstat, readlink, rename, rm, stat, symlink } from 'node:fs/promises'
import { basename, dirname, join, resolve } from 'node:path'

/** Hex SHA-1 of a file, streamed so a sixty megabyte video never sits in the heap. */
export function fileDigest(path: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const hash = createHash('sha1')
    createReadStream(path)
      .on('data', (chunk) => hash.update(chunk))
      .on('error', reject)
      .on('end', () => resolve(hash.digest('hex')))
  })
}

/**
 * Moves `path` to a sibling named by its content and leaves a symlink behind,
 * so two attachments with the same bytes resolve to one file. The renderer
 * keys its decoded images on the path, which is what makes forty copies of
 * one GIF cost one decode instead of forty. Returns the shared path, or
 * `path` itself where the filesystem allows neither a symlink nor a hard link.
 */
export async function shareByContent(path: string, extension: string): Promise<string> {
  const shared = join(dirname(path), `${await fileDigest(path)}${extension}`)
  if (shared === path) return path
  try {
    await lstat(shared)
    await rm(path, { force: true })
  } catch {
    await rename(path, shared)
  }
  try {
    await symlink(basename(shared), path)
  } catch {
    try {
      await link(shared, path)
    } catch {
      await copyFile(shared, path)
      return path
    }
  }
  return shared
}

/**
 * The file an attachment cache entry stands for: the shared file behind a
 * symlink, the entry itself otherwise, null when there is nothing there (a
 * link whose target was cleared counts as nothing). Only the link itself is
 * followed, not the directories above it: the renderer keys decoded images on
 * the path string, so the same file has to come back spelled the same way
 * whichever route asked for it.
 */
export async function cachedFile(path: string): Promise<string | null> {
  try {
    const info = await lstat(path)
    if (!info.isSymbolicLink()) return path
    const target = resolve(dirname(path), await readlink(path))
    await stat(target)
    return target
  } catch {
    return null
  }
}
