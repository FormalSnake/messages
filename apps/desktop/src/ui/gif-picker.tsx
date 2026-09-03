import { useEffect, useState } from 'react'
import { attachmentsDir, downloadGif, downloadGifPreview, type Gif } from '@messages/core'
import { C, RADIUS, S, TYPE } from './theme'
import { Icon } from './icons'
import { overlayShadow } from './primitives'
import { useShell } from './context'

const PANEL_WIDTH = 300
const PANEL_HEIGHT = 340
const CELL_HEIGHT = 84
const DEBOUNCE_MS = 300

function GifCell({ item, previewPath, onSelect }: { item: Gif; previewPath: string | undefined; onSelect: () => void }) {
  return (
    <div
      testId={`gif-${item.id}`}
      onClick={onSelect}
      style={{
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
    </div>
  )
}

export function GifPicker({ anchor, chatGuid, onClose }: { anchor: { x: number; y: number }; chatGuid: string; onClose: () => void }) {
  const shell = useShell()
  const { gifs, store } = shell
  const [query, setQuery] = useState('')
  const [committed, setCommitted] = useState('')
  const [items, setItems] = useState<Gif[]>([])
  const [loading, setLoading] = useState(true)
  const [previews, setPreviews] = useState<Record<string, string>>({})

  // Debounce keystrokes; onSubmit below commits immediately on Enter.
  useEffect(() => {
    const timer = setTimeout(() => setCommitted(query), DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [query])

  useEffect(() => {
    if (!gifs) return
    let cancelled = false
    setLoading(true)
    const term = committed.trim()
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

  if (!gifs) return null

  const pick = (item: Gif) => {
    onClose()
    void downloadGif(item, attachmentsDir).then((path) => store.sendAttachment(chatGuid, path))
  }

  return (
    <anchored deferred occlude priority={3} position={anchor} anchor="bottomLeft" fit="snap" snapMargin={S.x2}>
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

        <div
          testId="gif-grid"
          style={{
            flexGrow: 1,
            minHeight: 0,
            overflowY: 'scroll',
            display: loading || items.length === 0 ? 'flex' : 'grid',
            alignItems: loading || items.length === 0 ? 'center' : undefined,
            justifyContent: loading || items.length === 0 ? 'center' : undefined,
            gridTemplateColumns: loading || items.length === 0 ? undefined : 3,
            gap: loading || items.length === 0 ? undefined : S.x1,
          }}
        >
          {loading ? (
            <text style={{ ...TYPE.caption, color: C.secondary }}>Loading…</text>
          ) : items.length === 0 ? (
            <text style={{ ...TYPE.caption, color: C.secondary, textAlign: 'center' }}>{`No GIFs for "${committed.trim()}"`}</text>
          ) : (
            items.map((item) => <GifCell key={item.id} item={item} previewPath={previews[item.id]} onSelect={() => pick(item)} />)
          )}
        </div>
      </div>
    </anchored>
  )
}
