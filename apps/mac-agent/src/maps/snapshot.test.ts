import { describe, expect, test } from 'bun:test'
import { parseSnapshotRequest, type SnapshotRequest, SnapshotCache, snapshotKey, snapshotResponse } from './snapshot'

const params = (query: string) => new URLSearchParams(query)
const valid = 'lat=28.1235&lon=-15.4363&w=280&h=180'

describe('parseSnapshotRequest', () => {
  test('fills in scale, appearance and span and rounds the coordinate to five decimals', () => {
    expect(parseSnapshotRequest(params('lat=28.123456789&lon=-15.436349&w=280&h=180'))).toEqual({ lat: 28.12346, lon: -15.43635, w: 280, h: 180, scale: 2, dark: false, span: 800 })
  })

  test('reads every field it is given', () => {
    expect(parseSnapshotRequest(params(`${valid}&scale=3&dark=1&span=2500`))).toEqual({ lat: 28.1235, lon: -15.4363, w: 280, h: 180, scale: 3, dark: true, span: 2500 })
  })

  test.each([
    ['lon=-15.4&w=280&h=180', 'lat'],
    ['lat=91&lon=0&w=280&h=180', 'lat'],
    ['lat=abc&lon=0&w=280&h=180', 'lat'],
    ['lat=0&lon=181&w=280&h=180', 'lon'],
    ['lat=0&lon=0&w=280.5&h=180', 'w'],
    ['lat=0&lon=0&w=280&h=5000', 'h'],
    ['lat=0&lon=0&w=280&h=180&scale=4', 'scale'],
    ['lat=0&lon=0&w=280&h=180&dark=yes', 'dark'],
    ['lat=0&lon=0&w=280&h=180&span=10', 'span'],
  ])('refuses %s', (query, field) => {
    const result = parseSnapshotRequest(params(query))
    expect('error' in result && result.error.startsWith(field)).toBe(true)
  })
})

describe('snapshotKey', () => {
  test('keys on the rounded coordinate, the size and the appearance', () => {
    const a = parseSnapshotRequest(params('lat=28.1234561&lon=-15.4363&w=280&h=180')) as SnapshotRequest
    const b = parseSnapshotRequest(params('lat=28.1234559&lon=-15.4363&w=280&h=180')) as SnapshotRequest
    const dark = parseSnapshotRequest(params('lat=28.1234561&lon=-15.4363&w=280&h=180&dark=1')) as SnapshotRequest
    expect(snapshotKey(a)).toBe(snapshotKey(b))
    expect(snapshotKey(a)).not.toBe(snapshotKey(dark))
  })
})

describe('SnapshotCache', () => {
  const request = parseSnapshotRequest(params(valid)) as SnapshotRequest

  function counting(now: () => number) {
    let calls = 0
    const cache = new SnapshotCache(async () => new Uint8Array([++calls]), now, 1000)
    return { cache, calls: () => calls }
  }

  test('renders a key once while it is fresh and again once it is stale', async () => {
    let clock = 0
    const { cache, calls } = counting(() => clock)
    expect(await cache.get(request)).toEqual(new Uint8Array([1]))
    clock = 999
    expect(await cache.get(request)).toEqual(new Uint8Array([1]))
    clock = 1000
    expect(await cache.get(request)).toEqual(new Uint8Array([2]))
    expect(calls()).toBe(2)
  })

  test('shares one render between requests that arrive together', async () => {
    const { cache, calls } = counting(() => 0)
    const [first, second] = await Promise.all([cache.get(request), cache.get(request)])
    expect(first).toBe(second)
    expect(calls()).toBe(1)
  })

  test('does not cache a failure', async () => {
    let fail = true
    const cache = new SnapshotCache(async () => {
      if (fail) throw new Error('MapKit said no')
      return new Uint8Array([7])
    })
    await expect(cache.get(request)).rejects.toThrow('MapKit said no')
    fail = false
    expect(await cache.get(request)).toEqual(new Uint8Array([7]))
    expect(cache.size).toBe(1)
  })
})

describe('snapshotResponse', () => {
  test('answers a PNG', async () => {
    const response = await snapshotResponse(params(valid), new SnapshotCache(async () => new Uint8Array([137, 80, 78, 71])))
    expect(response.status).toBe(200)
    expect(response.headers.get('content-type')).toBe('image/png')
    expect(new Uint8Array(await response.arrayBuffer())).toEqual(new Uint8Array([137, 80, 78, 71]))
  })

  test('answers 400 without rendering when the request is invalid', async () => {
    let rendered = false
    const response = await snapshotResponse(
      params('lat=0&lon=0'),
      new SnapshotCache(async () => {
        rendered = true
        return new Uint8Array()
      }),
    )
    expect(response.status).toBe(400)
    expect(rendered).toBe(false)
  })

  test('answers 503 when the render fails', async () => {
    const response = await snapshotResponse(
      params(valid),
      new SnapshotCache(async () => {
        throw new Error('no MapKit')
      }),
    )
    expect(response.status).toBe(503)
    expect(((await response.json()) as { error: string }).error).toBe('no MapKit')
  })
})
