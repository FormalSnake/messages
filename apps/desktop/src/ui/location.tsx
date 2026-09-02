import { useEffect, useState } from 'react'
import { cacheDir, openExternal, relativeTime, tileFor, type FriendLocation, type Handle } from '@messages/core'
import { C, RADIUS, S, TYPE } from './theme'

const TILE_SIZE = 256
const TILE_ZOOM = 15
const PIN_SIZE = 12
const PIN_BORDER = 2

interface Tile {
  path: string
  px: number
  py: number
}

function openInMaps(location: FriendLocation): void {
  const { latitude, longitude } = location
  openExternal(`https://www.openstreetmap.org/?mlat=${latitude}&mlon=${longitude}#map=16/${latitude}/${longitude}`)
}

/**
 * The pin is laid out with a negative top margin rather than `position:
 * 'absolute'`: GPUI resolves an absolute child against the window, not this
 * box (see `GroupAvatar` in primitives.tsx for the same issue), so it would
 * land outside the tile instead of on it.
 */
function Pin({ px, py }: { px: number; py: number }) {
  return (
    <div
      style={{
        width: PIN_SIZE,
        height: PIN_SIZE,
        borderRadius: PIN_SIZE / 2,
        backgroundColor: C.accent,
        borderWidth: PIN_BORDER,
        borderColor: '#ffffff',
        flexShrink: 0,
        marginLeft: px - PIN_SIZE / 2,
        marginTop: py - PIN_SIZE / 2 - TILE_SIZE,
      }}
    />
  )
}

export function LocationCard({ handle, location }: { handle: Handle; location: FriendLocation }) {
  const [tile, setTile] = useState<Tile | null>(null)

  useEffect(() => {
    let cancelled = false
    setTile(null)
    void tileFor(location.latitude, location.longitude, TILE_ZOOM, cacheDir)
      .then((result) => {
        if (!cancelled) setTile(result)
      })
      .catch((error: unknown) => console.error(`findmy: tile fetch failed for ${handle.address}: ${String(error)}`))
    return () => {
      cancelled = true
    }
  }, [handle.address, location.latitude, location.longitude])

  const coordinateLabel = location.label ?? `${location.latitude.toFixed(4)}, ${location.longitude.toFixed(4)}`

  return (
    <div
      testId={`location-${handle.address}`}
      style={{ display: 'flex', flexDirection: 'column', gap: S.x2, paddingLeft: S.x2, paddingRight: S.x2, paddingTop: S.x1, paddingBottom: S.x3, flexShrink: 0 }}
    >
      <div
        style={{
          width: TILE_SIZE,
          height: TILE_SIZE,
          borderRadius: RADIUS.bubble,
          overflow: 'hidden',
          flexShrink: 0,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'flex-start',
          backgroundColor: C.raised,
        }}
      >
        {tile ? (
          <>
            <img src={tile.path} objectFit="cover" style={{ width: TILE_SIZE, height: TILE_SIZE, flexShrink: 0 }} />
            <Pin px={tile.px} py={tile.py} />
          </>
        ) : null}
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
        <text style={{ ...TYPE.caption, color: C.text }}>{coordinateLabel}</text>
        <text style={{ ...TYPE.micro, color: C.secondary }}>{relativeTime(location.timestamp)}</text>
      </div>
      <div
        tabIndex={0}
        onClick={() => openInMaps(location)}
        onKeyDown={(event) => {
          if (event.key === 'enter' || event.key === 'space') openInMaps(location)
        }}
        style={{ alignSelf: 'flex-start', cursor: 'pointer' }}
      >
        <text style={{ ...TYPE.caption, color: C.accent, borderBottomWidth: 1, borderColor: C.accent }}>Open in maps</text>
      </div>
    </div>
  )
}
