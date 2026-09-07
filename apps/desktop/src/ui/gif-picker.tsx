import { useEffect, useMemo, useState } from 'react'
import { attachmentsDir, downloadGif, downloadGifPreview, favoriteGifs, type Gif } from '@messages/core'
import { C, RADIUS, S, TYPE } from './theme'
import { HeartIcon, Icon } from './icons'
import { overlayShadow } from './primitives'
import { useShell } from './context'
import { useAppState } from './use-app-state'
import { DURATION, Fade } from './motion'

const PANEL_WIDTH = 300
const PANEL_HEIGHT = 340
const CELL_HEIGHT = 84
const DEBOUNCE_MS = 300

function GifCell({
  item,
  previewPath,
  favorited,
  onSelect,
  onToggleFavorite,
}: {
  item: Gif
  previewPath: string | undefined
  favorited: boolean
  onSelect: () => void
  onToggleFavorite: () => void
}) {
  return (
    <div
      testId={`gif-${item.id}`}
      onClick={onSelect}
      style={{
        position: 'relative',
        height: CELL_HEIGHT,
        borderRadius: RADIUS.control,
        overflow: 'hidden',
        backgroundColor: C.raised,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: 'pointer',
        hover: { opacity: 0.85 },
        active: { opacity: 0.7 },
      }}
    >
      {previewPath ? <img src={previewPath} objectFit="contain" style={{ width: '100%', height: '100%' }} /> : null}
      <div
        testId={`gif-favorite-${item.id}`}
        onClick={onToggleFavorite}
        style={{
          position: 'absolute',
          top: S.x1,
          right: S.x1,
          width: 20,
          height: 20,
          borderRadius: RADIUS.pill,
          backgroundColor: C.overlay,
          opacity: 0.85,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          cursor: 'pointer',
          hover: { opacity: 1 },
        }}
      >
        <HeartIcon size={11} color={favorited ? C.danger : C.onAccentSoft} filled={favorited} />
      </div>
    </div>
  )
}

export function GifPicker({ anchor, chatGuid, onClose }: { anchor: { x: number; y: number }; chatGuid: string; onClose: () => void }) {
  const shell = useShell()
  const { gifs, store } = shell
  const state = useAppState(store)
  const [query, setQuery] = useState('')
  const [committed, setCommitted] = useState('')
  const [items, setItems] = useState<Gif[]>([])
  const [loading, setLoading] = useState(true)
  const [previews, setPreviews] = useState<Record<string, string>>({})
  const [favoritePreviews, setFavoritePreviews] = useState<Record<string, string>>({})

  const term = committed.trim()
  const showFavorites = term.length === 0
  const favorites = useMemo(() => favoriteGifs(state.gifFavorites), [state.gifFavorites])
  const favoriteIds = useMemo(() => new Set(favorites.map((item) => item.id)), [favorites])

  // Debounce keystrokes; onSubmit below commits immediately on Enter.
  useEffect(() => {
    const timer = setTimeout(() => setCommitted(query), DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [query])

  useEffect(() => {
    if (!gifs) return
    let cancelled = false
    setLoading(true)
    const request = term ? gifs.search(term, 1) : gifs.trending(1)
    request
      .then((page) => {
        if (cancelled) return
        setItems(page.items)
        setLoading(false)
      })
      .catch(() => {
        if (cancelled) return
        setItems([])
        setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [committed, gifs])

  useEffect(() => {
    setPreviews({})
    let cancelled = false
    for (const item of items) {
      downloadGifPreview(item, attachmentsDir)
        .then((path) => {
          if (!cancelled) setPreviews((current) => ({ ...current, [item.id]: path }))
        })
        .catch(() => undefined)
    }
    return () => {
      cancelled = true
    }
  }, [items])

  // A cache hit resolves without a network call, which is what keeps favorites usable offline.
  useEffect(() => {
    let cancelled = false
    for (const favorite of favorites) {
      downloadGifPreview(favorite, attachmentsDir)
        .then((path) => {
          if (!cancelled) setFavoritePreviews((current) => ({ ...current, [favorite.id]: path }))
        })
        .catch(() => undefined)
    }
    return () => {
      cancelled = true
    }
  }, [favorites])

  if (!gifs) return null

  const pick = (item: Gif) => {
    onClose()
    void downloadGif(item, attachmentsDir).then((path) => store.sendAttachment(chatGuid, path))
  }

  return (
    <anchored deferred occlude priority={3} position={anchor} anchor="bottomLeft" fit="snap" snapMargin={S.x2}>
      <Fade enter={DURATION.fast}>
        <div
          testId="gif-picker"
          onMouseDownOutside={onClose}
          style={{
            display: 'flex',
            flexDirection: 'column',
            width: PANEL_WIDTH,
            height: PANEL_HEIGHT,
            gap: S.x2,
            padding: S.x2,
            borderRadius: RADIUS.menu,
            backgroundColor: C.overlay,
            borderWidth: 1,
            borderColor: C.overlayBorder,
            boxShadow: overlayShadow,
          }}
        >
          <div
            style={{
              display: 'flex',
              flexDirection: 'row',
              alignItems: 'center',
              gap: S.x1,
              flexShrink: 0,
              height: 28,
              paddingLeft: S.x2,
              paddingRight: S.x2,
              borderRadius: RADIUS.control,
              backgroundColor: C.canvas,
              borderWidth: 1,
              borderColor: C.separator,
            }}
          >
            <Icon name="search" size={13} color={C.tertiary} />
            <input
              testId="gif-search"
              value={query}
              autoFocus
              placeholder="Search KLIPY"
              onChange={(event) => setQuery(event.value ?? '')}
              onSubmit={() => setCommitted(query)}
              onKeyDown={(event) => {
                if (event.key === 'escape') onClose()
              }}
              theme={{ caret: C.accent, textMuted: C.tertiary }}
              style={{ flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, ...TYPE.body, color: C.text, backgroundColor: C.transparent, borderWidth: 0 }}
            />
          </div>

          <div testId="gif-grid" style={{ flexGrow: 1, minHeight: 0, overflowY: 'scroll', display: 'flex', flexDirection: 'column', gap: S.x3 }}>
            {showFavorites && favorites.length > 0 ? (
              <div style={{ display: 'flex', flexDirection: 'column', gap: S.x1, flexShrink: 0 }}>
                <text style={{ ...TYPE.caption, color: C.secondary }}>Favorites</text>
                <div style={{ display: 'grid', gridTemplateColumns: 3, gap: S.x1 }}>
                  {favorites.map((item) => (
                    <GifCell key={item.id} item={item} previewPath={favoritePreviews[item.id]} favorited onSelect={() => pick(item)} onToggleFavorite={() => store.toggleGifFavorite(item)} />
                  ))}
                </div>
              </div>
            ) : null}

            {loading ? (
              <div style={{ flexGrow: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                <text style={{ ...TYPE.caption, color: C.secondary }}>Loading…</text>
              </div>
            ) : items.length === 0 ? (
              <div style={{ flexGrow: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                <text style={{ ...TYPE.caption, color: C.secondary, textAlign: 'center' }}>{`No GIFs for "${term}"`}</text>
              </div>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: S.x1, flexShrink: 0 }}>
                {showFavorites ? <text style={{ ...TYPE.caption, color: C.secondary }}>Trending</text> : null}
                <div style={{ display: 'grid', gridTemplateColumns: 3, gap: S.x1 }}>
                  {items.map((item) => (
                    <GifCell
                      key={item.id}
                      item={item}
                      previewPath={previews[item.id]}
                      favorited={favoriteIds.has(item.id)}
                      onSelect={() => pick(item)}
                      onToggleFavorite={() => store.toggleGifFavorite(item)}
                    />
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      </Fade>
    </anchored>
  )
}
