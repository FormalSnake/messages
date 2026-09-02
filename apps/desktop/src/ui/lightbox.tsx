import { useEffect, useMemo, useState } from 'react'
import { useWindowSize } from '@gpuix/react'
import { formatBytes, openExternal, type Attachment, type Message } from '@messages/core'
import { C, RADIUS, S, TYPE } from './theme'
import { IconButton } from './primitives'
import { useShell, type LightboxTarget } from './context'
import { useAppState } from './use-app-state'

/** Margin kept between the image and every edge of the window. */
const MARGIN = 48
const CAPTION_HEIGHT = 40

export function Lightbox({ target }: { target: LightboxTarget }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const { width, height } = useWindowSize()
  const messages = state.messages[target.chatGuid] ?? []

  // Every non-hidden photo across the loaded thread, in the date order the
  // messages already carry.
  const images = useMemo(() => {
    const list: Array<{ attachment: Attachment; message: Message }> = []
    for (const message of messages) {
      for (const attachment of message.attachments) {
        if (!attachment.hidden && attachment.mime.startsWith('image/')) list.push({ attachment, message })
      }
    }
    return list
  }, [messages])

  const [current, setCurrent] = useState(target.attachmentGuid)
  const index = images.findIndex((item) => item.attachment.guid === current)
  const entry = index >= 0 ? images[index] : undefined
  const [failed, setFailed] = useState(false)

  useEffect(() => setFailed(false), [current])

  useEffect(() => {
    if (!entry || entry.attachment.localPath) return
    shell.store
      .attachmentSrc(entry.message.chatGuid, entry.message.guid, entry.attachment.guid, entry.attachment.name, entry.attachment.mime)
      .catch(() => setFailed(true))
  }, [entry, shell.store])

  const step = (delta: number) => {
    if (images.length === 0) return
    const next = images[(index + delta + images.length) % images.length]
    if (next) setCurrent(next.attachment.guid)
  }

  const src = entry?.attachment.localPath
  const availableW = Math.max(120, width - MARGIN * 2)
  const availableH = Math.max(120, height - MARGIN * 2 - CAPTION_HEIGHT - S.x2)
  const attachment = entry?.attachment
  // A real width/height scales the photo to fit; without one, fit a 4:3 box
  // instead (the ratio is all that matters, min() below only cares about it).
  const naturalW = attachment?.width && attachment.height ? attachment.width : 4
  const naturalH = attachment?.width && attachment.height ? attachment.height : 3
  const scale = Math.min(availableW / naturalW, availableH / naturalH)
  const dispW = Math.round(naturalW * scale)
  const dispH = Math.round(naturalH * scale)

  return (
    <anchored deferred occlude priority={5} position={{ x: 0, y: 0 }}>
      <div
        testId="lightbox"
        autoFocus
        tabIndex={-1}
        onKeyDown={(event) => {
          if (event.key === 'escape') shell.closeLightbox()
          else if (event.key === 'left' || event.key === 'k') step(-1)
          else if (event.key === 'right' || event.key === 'j') step(1)
        }}
        style={{
          width,
          height,
          backgroundColor: '#000000e6',
          pointerEvents: 'auto',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <div style={{ position: 'absolute', top: 20, right: 20, display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x2 }}>
          {src ? (
            <div
              testId="lightbox-open"
              onClick={() => openExternal(src)}
              style={{
                height: 28,
                paddingLeft: S.x3,
                paddingRight: S.x3,
                borderRadius: RADIUS.control,
                display: 'flex',
                alignItems: 'center',
                backgroundColor: '#ffffff1f',
                cursor: 'pointer',
                hover: { backgroundColor: '#ffffff33' },
              }}
            >
              <text style={{ ...TYPE.body, fontWeight: 600, color: '#ffffff' }}>Open</text>
            </div>
          ) : null}
          <IconButton testId="lightbox-close" icon="close" label="Close" onClick={shell.closeLightbox} color="#ffffff" hit={28} />
        </div>

        {entry ? (
          <div onMouseDownOutside={shell.closeLightbox} style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: S.x2 }}>
            {src ? (
              <img src={src} objectFit="contain" style={{ width: dispW, height: dispH }} />
            ) : (
              <div style={{ width: dispW, height: dispH, borderRadius: RADIUS.card, backgroundColor: '#ffffff14', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                <text style={{ ...TYPE.caption, color: '#ffffffb3' }}>{failed ? 'Could not load.' : 'Loading…'}</text>
              </div>
            )}
            <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center' }}>
              <text style={{ ...TYPE.caption, color: '#ffffff' }}>{entry.attachment.name}</text>
              <text style={{ ...TYPE.micro, color: '#ffffffa6' }}>{formatBytes(entry.attachment.bytes)}</text>
            </div>
          </div>
        ) : null}
      </div>
    </anchored>
  )
}
