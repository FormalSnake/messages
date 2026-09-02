import { describe, expect, test } from 'vitest'
import { parseUriList } from './clipboard'

const existing = new URL(import.meta.url).pathname // this test file itself, guaranteed to exist

describe('parseUriList', () => {
  test('decodes file:// entries that exist', () => {
    const text = `file://${encodeURI(existing)}\n`
    expect(parseUriList(text)).toEqual([existing])
  })

  test('drops entries that are not file:// uris', () => {
    expect(parseUriList('https://example.com/a.jpg\n')).toEqual([])
  })

  test('drops comments and blank lines', () => {
    const text = `# a comment\n\nfile://${encodeURI(existing)}\n`
    expect(parseUriList(text)).toEqual([existing])
  })

  test('drops file:// entries whose path does not exist', () => {
    expect(parseUriList('file:///no/such/file-xyz.jpg\n')).toEqual([])
  })
})
