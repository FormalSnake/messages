export interface ImageSize {
  width: number
  height: number
}

function u32be(bytes: Uint8Array, offset: number): number {
  return ((bytes[offset]! << 24) | (bytes[offset + 1]! << 16) | (bytes[offset + 2]! << 8) | bytes[offset + 3]!) >>> 0
}

function u16be(bytes: Uint8Array, offset: number): number {
  return (bytes[offset]! << 8) | bytes[offset + 1]!
}

function u16le(bytes: Uint8Array, offset: number): number {
  return bytes[offset]! | (bytes[offset + 1]! << 8)
}

function u32le(bytes: Uint8Array, offset: number): number {
  return (bytes[offset]! | (bytes[offset + 1]! << 8) | (bytes[offset + 2]! << 16) | (bytes[offset + 3]! << 24)) >>> 0
}

function ascii(bytes: Uint8Array, offset: number, length: number): string {
  return String.fromCharCode(...bytes.subarray(offset, offset + length))
}

function svgSize(source: string): ImageSize | null {
  const open = /<svg[^>]*>/i.exec(source)?.[0]
  if (!open) return null
  const attr = (name: string) => {
    const match = new RegExp(`\\b${name}\\s*=\\s*["']([^"']+)["']`, 'i').exec(open)?.[1]
    if (!match) return undefined
    const value = Number.parseFloat(match)
    return Number.isFinite(value) && value > 0 ? value : undefined
  }
  const width = attr('width')
  const height = attr('height')
  if (width && height) return { width: Math.round(width), height: Math.round(height) }
  const viewBox = /\bviewBox\s*=\s*["']([^"']+)["']/i.exec(open)?.[1]?.trim().split(/[\s,]+/).map(Number)
  if (viewBox && viewBox.length === 4 && viewBox[2]! > 0 && viewBox[3]! > 0) return { width: Math.round(viewBox[2]!), height: Math.round(viewBox[3]!) }
  return null
}

/** Pixel size from the container header alone: PNG, JPEG, GIF, WebP, BMP, or an SVG's declared size. */
export function imageSizeFromBytes(bytes: Uint8Array): ImageSize | null {
  if (bytes.length >= 24 && bytes[0] === 0x89 && ascii(bytes, 1, 3) === 'PNG') {
    return { width: u32be(bytes, 16), height: u32be(bytes, 20) }
  }
  if (bytes.length >= 10 && ascii(bytes, 0, 4) === 'GIF8') {
    return { width: u16le(bytes, 6), height: u16le(bytes, 8) }
  }
  if (bytes.length >= 4 && bytes[0] === 0xff && bytes[1] === 0xd8) {
    let offset = 2
    while (offset + 9 < bytes.length) {
      if (bytes[offset] !== 0xff) {
        offset += 1
        continue
      }
      const marker = bytes[offset + 1]!
      if (marker === 0xd8 || marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) {
        offset += 2
        continue
      }
      const length = u16be(bytes, offset + 2)
      const isSof = marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc
      if (isSof) return { height: u16be(bytes, offset + 5), width: u16be(bytes, offset + 7) }
      if (marker === 0xda) break
      offset += 2 + length
    }
    return null
  }
  if (bytes.length >= 30 && ascii(bytes, 0, 4) === 'RIFF' && ascii(bytes, 8, 4) === 'WEBP') {
    const chunk = ascii(bytes, 12, 4)
    if (chunk === 'VP8 ') return { width: u16le(bytes, 26) & 0x3fff, height: u16le(bytes, 28) & 0x3fff }
    if (chunk === 'VP8L') {
      const b = u32le(bytes, 21)
      return { width: (b & 0x3fff) + 1, height: ((b >> 14) & 0x3fff) + 1 }
    }
    if (chunk === 'VP8X') return { width: (bytes[24]! | (bytes[25]! << 8) | (bytes[26]! << 16)) + 1, height: (bytes[27]! | (bytes[28]! << 8) | (bytes[29]! << 16)) + 1 }
    return null
  }
  if (bytes.length >= 26 && ascii(bytes, 0, 2) === 'BM') {
    return { width: u32le(bytes, 18), height: Math.abs(u32le(bytes, 22) | 0) }
  }
  const head = ascii(bytes, 0, Math.min(bytes.length, 512))
  if (head.includes('<svg')) return svgSize(new TextDecoder().decode(bytes))
  return null
}

/** Size of a local image file or a `data:` URL, or null when the format is not recognised. */
export async function imageSize(source: string): Promise<ImageSize | null> {
  try {
    if (source.startsWith('data:')) {
      const comma = source.indexOf(',')
      if (comma < 0) return null
      const meta = source.slice(5, comma)
      const payload = source.slice(comma + 1)
      const bytes = meta.endsWith(';base64') ? Uint8Array.from(Buffer.from(payload, 'base64')) : new TextEncoder().encode(decodeURIComponent(payload))
      return imageSizeFromBytes(bytes)
    }
    const file = Bun.file(source)
    const head = await file.slice(0, 256 * 1024).arrayBuffer()
    return imageSizeFromBytes(new Uint8Array(head))
  } catch {
    return null
  }
}

/** Scale `size` down to fit inside a box, never up and never cropping. */
export function fitInside(size: ImageSize, maxWidth: number, maxHeight: number): ImageSize {
  const scale = Math.min(maxWidth / size.width, maxHeight / size.height, 1)
  return { width: Math.max(1, Math.round(size.width * scale)), height: Math.max(1, Math.round(size.height * scale)) }
}
