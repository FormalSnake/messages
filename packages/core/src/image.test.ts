import { describe, expect, it } from 'vitest'
import { fitInside, imageSize, imageSizeFromBytes } from './image'

function png(width: number, height: number): Uint8Array {
  const bytes = new Uint8Array(33)
  bytes.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52])
  new DataView(bytes.buffer).setUint32(16, width)
  new DataView(bytes.buffer).setUint32(20, height)
  return bytes
}

function jpeg(width: number, height: number): Uint8Array {
  // SOI, APP0 (16 bytes), SOF0 with one component
  const app0 = [0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0]
  const sof = [0xff, 0xc0, 0, 11, 8, height >> 8, height & 0xff, width >> 8, width & 0xff, 1, 1, 0x11, 0]
  return Uint8Array.from([0xff, 0xd8, ...app0, ...sof, 0xff, 0xda])
}

describe('imageSizeFromBytes', () => {
  it('reads png', () => expect(imageSizeFromBytes(png(600, 1300))).toEqual({ width: 600, height: 1300 }))
  it('reads jpeg after an APP0 segment', () => expect(imageSizeFromBytes(jpeg(1080, 1920))).toEqual({ width: 1080, height: 1920 }))
  it('reads gif', () => {
    const bytes = Uint8Array.from([...'GIF89a'].map((c) => c.charCodeAt(0)).concat([0xf4, 0x01, 0x2c, 0x01, 0, 0, 0]))
    expect(imageSizeFromBytes(bytes)).toEqual({ width: 500, height: 300 })
  })
  it('reads svg width and height', () => {
    const svg = new TextEncoder().encode('<svg xmlns="http://www.w3.org/2000/svg" width="600" height="1300" viewBox="0 0 600 1300"></svg>')
    expect(imageSizeFromBytes(svg)).toEqual({ width: 600, height: 1300 })
  })
  it('returns null for unknown bytes', () => expect(imageSizeFromBytes(new Uint8Array([1, 2, 3, 4]))).toBeNull())
})

describe('imageSize', () => {
  it('handles data urls', async () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 200"></svg>'
    expect(await imageSize(`data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}`)).toEqual({ width: 400, height: 200 })
  })
})

describe('fitInside', () => {
  it('scales tall images down to the height cap', () => expect(fitInside({ width: 600, height: 1300 }, 320, 420)).toEqual({ width: 194, height: 420 }))
  it('never scales up', () => expect(fitInside({ width: 100, height: 50 }, 320, 420)).toEqual({ width: 100, height: 50 }))
})
