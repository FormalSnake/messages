import { describe, expect, test } from 'bun:test'
import { chacha20, chacha20Poly1305Decrypt, chacha20Poly1305Encrypt, poly1305 } from './chacha'

function hex(text: string): Uint8Array {
  const clean = text.replace(/\s+/g, '')
  const out = new Uint8Array(clean.length / 2)
  for (let index = 0; index < out.length; index += 1) out[index] = Number.parseInt(clean.substr(index * 2, 2), 16)
  return out
}

// RFC 8439 §2.4.2
describe('chacha20 (RFC 8439 §2.4.2)', () => {
  test('encrypts the sunscreen plaintext', () => {
    const key = hex('00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f 10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f')
    const nonce = hex('00 00 00 00 00 00 00 4a 00 00 00 00')
    const plaintext = new TextEncoder().encode(
      "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.",
    )
    const expected = hex(`
      6e 2e 35 9a 25 68 f9 80 41 ba 07 28 dd 0d 69 81
      e9 7e 7a ec 1d 43 60 c2 0a 27 af cc fd 9f ae 0b
      f9 1b 65 c5 52 47 33 ab 8f 59 3d ab cd 62 b3 57
      16 39 d6 24 e6 51 52 ab 8f 53 0c 35 9f 08 61 d8
      07 ca 0d bf 50 0d 6a 61 56 a3 8e 08 8a 22 b6 5e
      52 bc 51 4d 16 cc f8 06 81 8c e9 1a b7 79 37 36
      5a f9 0b bf 74 a3 5b e6 b4 0b 8e ed f2 78 5e 42
      87 4d
    `)
    expect(chacha20(key, 1, nonce, plaintext)).toEqual(expected)
  })
})

// RFC 8439 §2.5.2
describe('poly1305 (RFC 8439 §2.5.2)', () => {
  test('tags the Cryptographic Forum Research Group message', () => {
    const key = hex('85 d6 be 78 57 55 6d 33 7f 44 52 fe 42 d5 06 a8 01 03 80 8a fb 0d b2 fd 4a bf f6 af 41 49 f5 1b')
    const message = new TextEncoder().encode('Cryptographic Forum Research Group')
    const expected = hex('a8 06 1d c1 30 51 36 c6 c2 2b 8b af 0c 01 27 a9')
    expect(poly1305(key, message)).toEqual(expected)
  })
})

// RFC 8439 §2.8.2 (AEAD_CHACHA20_POLY1305)
describe('chacha20-poly1305 AEAD (RFC 8439 §2.8.2)', () => {
  const key = hex('80 81 82 83 84 85 86 87 88 89 8a 8b 8c 8d 8e 8f 90 91 92 93 94 95 96 97 98 99 9a 9b 9c 9d 9e 9f')
  // Nonce = 32-bit fixed constant (07 00 00 00) followed by the 64-bit IV from the vector.
  const nonce = hex('07 00 00 00 40 41 42 43 44 45 46 47')
  const aad = hex('50 51 52 53 c0 c1 c2 c3 c4 c5 c6 c7')
  const plaintext = new TextEncoder().encode(
    "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.",
  )
  const ciphertext = hex(`
    d3 1a 8d 34 64 8e 60 db 7b 86 af bc 53 ef 7e c2
    a4 ad ed 51 29 6e 08 fe a9 e2 b5 a7 36 ee 62 d6
    3d be a4 5e 8c a9 67 12 82 fa fb 69 da 92 72 8b
    1a 71 de 0a 9e 06 0b 29 05 d6 a5 b6 7e cd 3b 36
    92 dd bd 7f 2d 77 8b 8c 98 03 ae e3 28 09 1b 58
    fa b3 24 e4 fa d6 75 94 55 85 80 8b 48 31 d7 bc
    3f f4 de f0 8e 4b 7a 9d e5 76 d2 65 86 ce c6 4b
    61 16
  `)
  const tag = hex('1a e1 0b 59 4f 09 e2 6a 7e 90 2e cb d0 60 06 91')

  test('encrypts and authenticates to the RFC ciphertext and tag', () => {
    const sealed = chacha20Poly1305Encrypt(key, nonce, plaintext, aad)
    expect(sealed.subarray(0, ciphertext.length)).toEqual(ciphertext)
    expect(sealed.subarray(ciphertext.length)).toEqual(tag)
  })

  test('decrypts the RFC sealed vector back to the plaintext', () => {
    const sealed = new Uint8Array([...ciphertext, ...tag])
    expect(chacha20Poly1305Decrypt(key, nonce, sealed, aad)).toEqual(plaintext)
  })

  test('rejects a flipped ciphertext byte', () => {
    const sealed = new Uint8Array([...ciphertext, ...tag])
    sealed[0] = sealed[0]! ^ 0xff
    expect(() => chacha20Poly1305Decrypt(key, nonce, sealed, aad)).toThrow(/authentication failed/)
  })
})
