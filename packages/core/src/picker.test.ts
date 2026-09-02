import { describe, expect, test } from 'vitest'
import { parsePortalResponse } from './picker'

describe('parsePortalResponse', () => {
  test('extracts and decodes uris from a success response', () => {
    const line =
      "/org/freedesktop/portal/desktop/request/1_90/messagesabc: org.freedesktop.portal.Request.Response (uint32 0, {'uris': <['file:///home/user/a%20b.jpg', 'file:///home/user/c.png']>})"
    expect(parsePortalResponse(line)).toEqual(['/home/user/a b.jpg', '/home/user/c.png'])
  })

  test('returns a single path', () => {
    const line = "... org.freedesktop.portal.Request.Response (uint32 0, {'uris': <['file:///home/user/only.jpg']>})"
    expect(parsePortalResponse(line)).toEqual(['/home/user/only.jpg'])
  })

  test('returns [] when the user cancelled', () => {
    const line = "... org.freedesktop.portal.Request.Response (uint32 1, {})"
    expect(parsePortalResponse(line)).toEqual([])
  })

  test('returns [] when the dialog ended another way', () => {
    const line = "... org.freedesktop.portal.Request.Response (uint32 2, {})"
    expect(parsePortalResponse(line)).toEqual([])
  })

  test('returns [] for a line with no uris entry', () => {
    expect(parsePortalResponse("... org.freedesktop.portal.Request.Response (uint32 0, {'choices': <[]>})")).toEqual([])
  })
})
