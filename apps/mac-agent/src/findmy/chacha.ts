/**
 * ChaCha20-Poly1305 (RFC 8439), because Bun's node:crypto does not carry the
 * cipher and WebCrypto refuses it. Find My seals every `.data` cache with it.
 */

function rotl(value: number, bits: number): number {
  return ((value << bits) | (value >>> (32 - bits))) >>> 0
}

const CONSTANTS = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574]

function block(key: Uint32Array, counter: number, nonce: Uint32Array, out: Uint8Array): void {
  const state = new Uint32Array(16)
  state.set(CONSTANTS, 0)
  state.set(key, 4)
  state[12] = counter >>> 0
  state.set(nonce, 13)
  const working = state.slice()
  for (let round = 0; round < 10; round += 1) {
    quarter(working, 0, 4, 8, 12)
    quarter(working, 1, 5, 9, 13)
    quarter(working, 2, 6, 10, 14)
    quarter(working, 3, 7, 11, 15)
    quarter(working, 0, 5, 10, 15)
    quarter(working, 1, 6, 11, 12)
    quarter(working, 2, 7, 8, 13)
    quarter(working, 3, 4, 9, 14)
  }
  for (let index = 0; index < 16; index += 1) {
    const word = (working[index]! + state[index]!) >>> 0
    out[index * 4] = word & 0xff
    out[index * 4 + 1] = (word >>> 8) & 0xff
    out[index * 4 + 2] = (word >>> 16) & 0xff
    out[index * 4 + 3] = (word >>> 24) & 0xff
  }
}

function quarter(state: Uint32Array, a: number, b: number, c: number, d: number): void {
  state[a] = (state[a]! + state[b]!) >>> 0
  state[d] = rotl(state[d]! ^ state[a]!, 16)
  state[c] = (state[c]! + state[d]!) >>> 0
  state[b] = rotl(state[b]! ^ state[c]!, 12)
  state[a] = (state[a]! + state[b]!) >>> 0
  state[d] = rotl(state[d]! ^ state[a]!, 8)
  state[c] = (state[c]! + state[d]!) >>> 0
  state[b] = rotl(state[b]! ^ state[c]!, 7)
}

function words(bytes: Uint8Array): Uint32Array {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const out = new Uint32Array(bytes.byteLength / 4)
  for (let index = 0; index < out.length; index += 1) out[index] = view.getUint32(index * 4, true)
  return out
}

export function chacha20(key: Uint8Array, counter: number, nonce: Uint8Array, input: Uint8Array): Uint8Array {
  const keyWords = words(key)
  const nonceWords = words(nonce)
  const out = new Uint8Array(input.length)
  const stream = new Uint8Array(64)
  for (let offset = 0; offset < input.length; offset += 64) {
    block(keyWords, counter + offset / 64, nonceWords, stream)
    const end = Math.min(64, input.length - offset)
    for (let index = 0; index < end; index += 1) out[offset + index] = input[offset + index]! ^ stream[index]!
  }
  return out
}

const P = (1n << 130n) - 5n
const MASK128 = (1n << 128n) - 1n

function littleEndian(bytes: Uint8Array): bigint {
  let value = 0n
  for (let index = bytes.length - 1; index >= 0; index -= 1) value = (value << 8n) | BigInt(bytes[index]!)
  return value
}

/** RFC 8439 §2.5. BigInt rather than 26-bit limbs: a cache file is tens of kilobytes, not a stream. */
export function poly1305(key: Uint8Array, message: Uint8Array): Uint8Array {
  const r = littleEndian(key.subarray(0, 16)) & 0x0ffffffc0ffffffc0ffffffc0fffffffn
  const s = littleEndian(key.subarray(16, 32))
  let accumulator = 0n
  for (let offset = 0; offset < message.length; offset += 16) {
    const chunk = message.subarray(offset, Math.min(offset + 16, message.length))
    accumulator = (accumulator + littleEndian(chunk) + (1n << BigInt(chunk.length * 8))) % P
    accumulator = (accumulator * r) % P
  }
  const tag = (accumulator + s) & MASK128
  const out = new Uint8Array(16)
  let rest = tag
  for (let index = 0; index < 16; index += 1) {
    out[index] = Number(rest & 0xffn)
    rest >>= 8n
  }
  return out
}

function pad16(length: number): number {
  return length % 16 === 0 ? 0 : 16 - (length % 16)
}

function tagFor(key: Uint8Array, nonce: Uint8Array, ciphertext: Uint8Array, aad: Uint8Array): Uint8Array {
  const otk = chacha20(key, 0, nonce, new Uint8Array(32))
  const mac = new Uint8Array(aad.length + pad16(aad.length) + ciphertext.length + pad16(ciphertext.length) + 16)
  let cursor = 0
  mac.set(aad, cursor)
  cursor += aad.length + pad16(aad.length)
  mac.set(ciphertext, cursor)
  cursor += ciphertext.length + pad16(ciphertext.length)
  const lengths = new DataView(mac.buffer, mac.byteOffset + cursor, 16)
  lengths.setBigUint64(0, BigInt(aad.length), true)
  lengths.setBigUint64(8, BigInt(ciphertext.length), true)
  return poly1305(otk, mac)
}

function equal(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false
  let diff = 0
  for (let index = 0; index < a.length; index += 1) diff |= a[index]! ^ b[index]!
  return diff === 0
}

export function chacha20Poly1305Encrypt(key: Uint8Array, nonce: Uint8Array, plaintext: Uint8Array, aad: Uint8Array = new Uint8Array(0)): Uint8Array {
  const ciphertext = chacha20(key, 1, nonce, plaintext)
  const out = new Uint8Array(ciphertext.length + 16)
  out.set(ciphertext, 0)
  out.set(tagFor(key, nonce, ciphertext, aad), ciphertext.length)
  return out
}

/** `sealed` is ciphertext followed by the 16-byte tag. Throws when the tag does not verify. */
export function chacha20Poly1305Decrypt(key: Uint8Array, nonce: Uint8Array, sealed: Uint8Array, aad: Uint8Array = new Uint8Array(0)): Uint8Array {
  if (key.length !== 32) throw new Error(`chacha20-poly1305: key is ${key.length} bytes, expected 32`)
  if (nonce.length !== 12) throw new Error(`chacha20-poly1305: nonce is ${nonce.length} bytes, expected 12`)
  if (sealed.length < 16) throw new Error('chacha20-poly1305: sealed data is shorter than the tag')
  const ciphertext = sealed.subarray(0, sealed.length - 16)
  const tag = sealed.subarray(sealed.length - 16)
  if (!equal(tagFor(key, nonce, ciphertext, aad), tag)) throw new Error('chacha20-poly1305: authentication failed (wrong key)')
  return chacha20(key, 1, nonce, ciphertext)
}
