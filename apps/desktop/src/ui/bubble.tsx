import { useEffect, useRef, useState } from 'react'
import type { EventPayload } from '@gpuix/react'
import { type Attachment, type Chat, type Message, type MessagePart, type RichRun, fitInside } from '@messages/core'
import { attachmentsDir, formatBytes } from '@messages/core'
import { isAudioPlaying, openExternal, playAudio, splitLinks, stopAudio } from '@messages/core'
import playSource from 'lucide-static/icons/play.svg' with { type: 'text' }
import stopSource from 'lucide-static/icons/square.svg' with { type: 'text' }
import { BUBBLE_MAX_WIDTH, C, RADIUS, S, TYPE } from './theme'
import { Icon } from './icons'
import { useShell } from './context'

const AUTO_DOWNLOAD_BYTES = 25 * 1024 * 1024
const AUTO_DOWNLOAD_VIDEO_BYTES = 60 * 1024 * 1024
const STICKER_WIDTH = 110
const PREVIEW_WIDTH = 280
/** Cap for an inline photo or video poster; a bubble stays readable past it. */
const MEDIA_MAX_WIDTH = 320
const MEDIA_MAX_HEIGHT = 420
/** Used to box an image or poster when the server has no width/height for it. */
const FALLBACK_RATIO = 4 / 3
const PLAY_CIRCLE = 44
/** Left for the avatar gutter a group thread puts beside every received row. */
const GROUP_GUTTER = 36
const EMOJI_ONLY = /^(?:\p{Extended_Pictographic}️?(?:‍\p{Extended_Pictographic}️?)*\s*){1,3}$/u
/** GPUI only wraps at whitespace, so an unbroken run past this length pushes the bubble off screen. */
const LONG_TOKEN = /\S{33,}/g

/**
 * icons.tsx carries no transport controls. Bake the same way it does, since
 * GPUI tints an icon as a mask and never resolves currentColor. Lucide's 2px
 * stroke stays: it is the weight that holds up inside a small round button.
 */
function transportGlyph(source: string): string {
  return source.replace(/currentColor/g, '#000')
}

const PLAY_GLYPH = transportGlyph(playSource)
const STOP_GLYPH = transportGlyph(stopSource)

/** Inserts U+200B every 16 characters inside a token longer than 32, so the native renderer has somewhere to wrap it. */
function softWrap(text: string): string {
  return text.replace(LONG_TOKEN, (token) => {
    const chunks: string[] = []
    for (let index = 0; index < token.length; index += 16) chunks.push(token.slice(index, index + 16))
    return chunks.join('\u200B')
  })
}

/**
 * Box for a photo or video poster: the image scaled to fit inside cap x 420
 * without cropping, so the box always has the image's own aspect and the
 * list row height is exact. Unknown sizes get a 4:3 placeholder until the
 * store reads the file header.
 */
function mediaDims(attachment: Attachment, cap: number): { width: number; height: number } {
  if (attachment.width && attachment.height) return fitInside({ width: attachment.width, height: attachment.height }, cap, MEDIA_MAX_HEIGHT)
  return { width: cap, height: Math.round(cap / FALLBACK_RATIO) }
}

export function ImageAttachment({ attachment, message, maxWidth }: { attachment: Attachment; message: Message; maxWidth?: number }) {
  const shell = useShell()
  const [failed, setFailed] = useState(false)
  const src = attachment.localPath
  const { width, height } = mediaDims(attachment, Math.min(maxWidth ?? MEDIA_MAX_WIDTH, MEDIA_MAX_WIDTH))
  const sizeKnown = Boolean(attachment.width && attachment.height)
  // Fetch the file, or, for a cached file whose pixel size chat.db did not record, just its header.
  const shouldFetch = ((!src && attachment.bytes <= AUTO_DOWNLOAD_BYTES) || (Boolean(src) && !sizeKnown)) && !failed
  useEffect(() => {
    if (!shouldFetch) return
    shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => setFailed(true))
  }, [shouldFetch, shell.store, message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime])
  if (src) {
    return (
      <div
        onClick={() => shell.openLightbox({ chatGuid: message.chatGuid, attachmentGuid: attachment.guid })}
        style={{ width, height, cursor: 'pointer', borderRadius: RADIUS.bubble, overflow: 'hidden', borderWidth: 1, borderColor: '#ffffff1a', backgroundColor: C.received }}
      >
        <img src={src} objectFit="contain" style={{ width, height }} />
      </div>
    )
  }
  return (
    <div
      onClick={() => {
        setFailed(false)
        shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => setFailed(true))
      }}
      style={{ width, height, borderRadius: RADIUS.bubble, backgroundColor: C.received, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 6, cursor: 'pointer' }}
    >
      <Icon name="image" size={22} color={C.secondary} />
      <text style={{ ...TYPE.caption, color: C.secondary }}>{failed ? 'Could not load. Click to retry.' : shouldFetch ? 'Loading…' : `Click to download (${formatBytes(attachment.bytes)})`}</text>
    </div>
  )
}

export function FileAttachment({ attachment, message, fromMe }: { attachment: Attachment; message: Message; fromMe: boolean }) {
  const shell = useShell()
  const open = async () => {
    const local = attachment.localPath ?? (await shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => undefined))
    if (local) openExternal(local)
  }
  return (
    <div
      onClick={() => void open()}
      style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10, paddingLeft: 12, paddingRight: 14, paddingTop: 10, paddingBottom: 10, borderRadius: RADIUS.bubble, backgroundColor: fromMe ? C.imessage : C.received, cursor: 'pointer', maxWidth: 320 }}
    >
      <Icon name={message.isAudio ? 'audio' : 'file'} size={20} color={fromMe ? C.onAccent : C.text} />
      <div style={{ display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <text style={{ ...TYPE.body, fontWeight: 600, color: fromMe ? C.onAccent : C.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{message.isAudio ? 'Audio message' : attachment.name}</text>
        <text style={{ ...TYPE.micro, color: fromMe ? '#ffffffb3' : C.secondary }}>{formatBytes(attachment.bytes)}</text>
      </div>
    </div>
  )
}

export function BubbleText({ text, color }: { text: string; color: string }) {
  const lines = text.split('\n')
  return (
    <div style={{ display: 'flex', flexDirection: 'column' }}>
      {lines.map((line, index) => (
        <text key={index} style={{ ...TYPE.bubble, color }}>
          {line.length ? softWrap(line) : ' '}
        </text>
      ))}
    </div>
  )
}

/** m:ss, the way Messages labels an audio message. */
function formatDuration(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000))
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`
}

function hostOf(url: string): string {
  return /^(?:https?:\/\/)?([^/?#]+)/i.exec(url)?.[1]?.replace(/^www\./i, '') ?? url
}

function sameUrl(a: string, b: string): boolean {
  const strip = (value: string) => value.trim().replace(/^https?:\/\//i, '').replace(/\/+$/, '').toLowerCase()
  return strip(a) === strip(b)
}

type Chunk = { run: RichRun; text: string }

/**
 * `<text>` cannot nest styled spans, so a styled part is a wrapping row of one
 * `<text>` per word. Each word keeps its own trailing space, since the row
 * breaks between children and a gap would also space out punctuation.
 */
function toLines(runs: RichRun[]): Chunk[][] {
  const lines: Chunk[][] = [[]]
  for (const run of runs) {
    const segments = run.text.split('\n')
    segments.forEach((segment, index) => {
      if (index > 0) lines.push([])
      const line = lines[lines.length - 1]!
      for (const word of segment.match(/\S+\s*|\s+/g) ?? []) line.push({ run, text: word })
    })
  }
  return lines
}

/**
 * The attributed body only marks links someone typed as a link, so bare URLs
 * still need the same pass the plain text path gives them.
 */
function linkify(runs: RichRun[]): RichRun[] {
  const out: RichRun[] = []
  for (const run of runs) {
    if (run.link || run.mention) {
      out.push(run)
      continue
    }
    for (const segment of splitLinks(run.text)) {
      out.push(segment.kind === 'link' ? { ...run, text: segment.value, link: segment.href } : { ...run, text: segment.value })
    }
  }
  return out
}

/** A run of text, or a link pulled out to become its own bubble. */
type TextAtom = { kind: 'text'; value: string } | { kind: 'link'; href: string }
type RunAtom = { kind: 'text'; runs: RichRun[] } | { kind: 'link'; href: string }

/** Splits plain message text at every URL, the way Messages pulls a link out from under the text it sits in. */
function splitTextAtLinks(text: string): TextAtom[] {
  const atoms: TextAtom[] = []
  for (const segment of splitLinks(text)) {
    if (segment.kind === 'link') atoms.push({ kind: 'link', href: segment.href })
    else if (segment.value.trim().length > 0) atoms.push({ kind: 'text', value: segment.value })
  }
  return atoms
}

/** Same split for a rich-text part: an explicit link attribute or an auto-detected URL both become their own atom. */
function splitRunsAtLinks(runs: RichRun[]): RunAtom[] {
  const atoms: RunAtom[] = []
  let pending: RichRun[] = []
  const flush = () => {
    if (pending.some((run) => run.text.trim().length > 0)) atoms.push({ kind: 'text', runs: pending })
    pending = []
  }
  for (const run of linkify(runs)) {
    if (run.link) {
      flush()
      atoms.push({ kind: 'link', href: run.link })
      continue
    }
    pending.push(run)
  }
  flush()
  return atoms
}

const EFFECT_SIZE: Partial<Record<NonNullable<RichRun['effect']>, { fontSize: number; lineHeight: number }>> = {
  big: { fontSize: 22, lineHeight: 28 },
  small: { fontSize: 11, lineHeight: 15 },
}

function RunChunk({ chunk, fromMe, color }: { chunk: Chunk; fromMe: boolean; color: string }) {
  const { run, text } = chunk
  // Accent on a blue bubble would be the bubble itself, so mentions and links
  // take white there instead.
  const accent = fromMe ? C.onAccent : C.accent
  const size = (run.effect && EFFECT_SIZE[run.effect]) ?? { fontSize: TYPE.bubble.fontSize, lineHeight: TYPE.bubble.lineHeight }
  const style = {
    ...size,
    color: run.mention || run.link || run.underline ? accent : color,
    fontWeight: run.bold || run.mention ? 700 : undefined,
    // GPUI has no italic face selection: 0.9 opacity is the closest weight-safe stand-in.
    opacity: run.italic ? 0.9 : undefined,
    // No text-decoration in gpuix styles, so the rule is the element's own edge.
    ...(run.underline || run.link ? { borderBottomWidth: 1, borderColor: accent } : {}),
    ...(run.link ? { cursor: 'pointer' as const, hover: { opacity: 0.8 } } : {}),
  }
  const label = (
    <text onClick={run.link ? () => openExternal(run.link!) : undefined} style={style}>
      {softWrap(text)}
    </text>
  )
  if (!run.strike) return label
  // And a strikethrough has no edge to borrow, so it is a hairline over the word.
  return (
    <div style={{ position: 'relative', display: 'flex', flexDirection: 'row' }}>
      {label}
      <div style={{ position: 'absolute', left: 0, right: 0, top: Math.round(size.lineHeight / 2), height: 1, backgroundColor: color, opacity: 0.7 }} />
    </div>
  )
}

function RichText({ runs, fromMe, color }: { runs: RichRun[]; fromMe: boolean; color: string }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column' }}>
      {toLines(runs).map((line, index) => (
        <div key={index} style={{ display: 'flex', flexDirection: 'row', flexWrap: 'wrap', alignItems: 'flex-end' }}>
          {line.length === 0 ? (
            <text style={{ ...TYPE.bubble, color }}> </text>
          ) : (
            line.map((chunk, chunkIndex) => <RunChunk key={chunkIndex} chunk={chunk} fromMe={fromMe} color={color} />)
          )}
        </div>
      ))}
    </div>
  )
}

/** Row-level behaviour the thread attaches to every surface: run-aware corners, menus, hover time. */
export interface BubbleHandlers {
  radius?: Partial<Record<'borderTopLeftRadius' | 'borderTopRightRadius' | 'borderBottomLeftRadius' | 'borderBottomRightRadius', number>>
  dimmed?: boolean
  paddingTop?: number
  onClick?: (event: EventPayload) => void
  onMenu?: (event: EventPayload) => void
  onAttachmentMenu?: (attachment: Attachment, event: EventPayload) => void
  onHover?: (hovered: boolean) => void
}

function TextBubble({ children, fill, fromMe, handlers }: { children: React.ReactNode; fill: string; fromMe: boolean; handlers?: BubbleHandlers }) {
  return (
    <div
      onClick={handlers?.onClick}
      onAuxClick={handlers?.onMenu}
      onMouseEnter={handlers?.onHover ? () => handlers.onHover?.(true) : undefined}
      onMouseLeave={handlers?.onHover ? () => handlers.onHover?.(false) : undefined}
      style={{
        paddingLeft: 12,
        paddingRight: 12,
        paddingTop: handlers?.paddingTop ?? 7,
        paddingBottom: 7,
        borderRadius: RADIUS.bubble,
        ...handlers?.radius,
        backgroundColor: fill,
        opacity: handlers?.dimmed ? 0.7 : 1,
        alignSelf: fromMe ? 'flex-end' : 'flex-start',
        maxWidth: '100%',
        cursor: 'default',
      }}
    >
      {children}
    </div>
  )
}

function StickerAttachment({ attachment, message }: { attachment: Attachment; message: Message }) {
  const shell = useShell()
  const [failed, setFailed] = useState(false)
  const src = attachment.localPath
  const height = attachment.width && attachment.height ? Math.round((STICKER_WIDTH * attachment.height) / attachment.width) : STICKER_WIDTH
  useEffect(() => {
    if (src || failed) return
    shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => setFailed(true))
  }, [src, failed, shell.store, message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime])
  if (!src) return <div style={{ width: STICKER_WIDTH, height }} />
  return <img src={src} objectFit="contain" style={{ width: STICKER_WIDTH, height }} />
}

/** `<cache>/<guid>.poster.jpg`, the frame ffmpeg pulls from the downloaded video. */
function posterCachePath(guid: string): string {
  return `${attachmentsDir}/${guid}.poster.jpg`
}

/**
 * Grabs the frame at 0.5s (skipping a black opening frame) and scales it to
 * 640 wide. Silently gives up when ffmpeg is not on PATH or the run fails;
 * the caller falls back to a plain dark box.
 */
async function generatePoster(videoPath: string, guid: string): Promise<string | null> {
  const target = posterCachePath(guid)
  if (await Bun.file(target).exists()) return target
  if (!Bun.which('ffmpeg')) return null
  try {
    const proc = Bun.spawn(['ffmpeg', '-y', '-ss', '0.5', '-i', videoPath, '-frames:v', '1', '-vf', 'scale=640:-1', target], { stdout: 'ignore', stderr: 'ignore' })
    const code = await proc.exited
    return code === 0 && (await Bun.file(target).exists()) ? target : null
  } catch {
    return null
  }
}

function VideoAttachment({ attachment, message, fromMe, maxWidth }: { attachment: Attachment; message: Message; fromMe: boolean; maxWidth: number }) {
  const shell = useShell()
  const [failed, setFailed] = useState(false)
  const [poster, setPoster] = useState<string | null>(null)
  const posterTried = useRef(false)
  const src = attachment.localPath
  const { width, height } = mediaDims(attachment, Math.min(maxWidth, MEDIA_MAX_WIDTH))
  const shouldFetch = !src && attachment.bytes <= AUTO_DOWNLOAD_VIDEO_BYTES && !failed

  useEffect(() => {
    if (!shouldFetch) return
    shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => setFailed(true))
  }, [shouldFetch, shell.store, message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime])

  useEffect(() => {
    if (!src || posterTried.current) return
    posterTried.current = true
    void generatePoster(src, attachment.guid).then(setPoster)
  }, [src, attachment.guid])

  const open = async () => {
    const local = attachment.localPath ?? (await shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => undefined))
    if (local) openExternal(local)
  }

  if (!src) {
    return (
      <div
        onClick={() => {
          setFailed(false)
          shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => setFailed(true))
        }}
        style={{ width, height, borderRadius: RADIUS.bubble, backgroundColor: C.received, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 6, cursor: 'pointer' }}
      >
        <Icon name="video" size={22} color={C.secondary} />
        <text style={{ ...TYPE.caption, color: C.secondary }}>{failed ? 'Could not load. Click to retry.' : shouldFetch ? 'Loading…' : `Click to download (${formatBytes(attachment.bytes)})`}</text>
      </div>
    )
  }

  return (
    <div
      onClick={() => void open()}
      style={{ width, height, borderRadius: RADIUS.bubble, overflow: 'hidden', position: 'relative', cursor: 'pointer', backgroundColor: '#1c1c1e', hover: { opacity: 0.94 } }}
    >
      {poster ? <img src={poster} objectFit="cover" style={{ width, height }} /> : null}
      <div
        style={{
          position: 'absolute',
          top: Math.round(height / 2 - PLAY_CIRCLE / 2),
          left: Math.round(width / 2 - PLAY_CIRCLE / 2),
          width: PLAY_CIRCLE,
          height: PLAY_CIRCLE,
          borderRadius: PLAY_CIRCLE / 2,
          backgroundColor: '#00000080',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <svg source={PLAY_GLYPH} style={{ width: 18, height: 18, color: '#ffffff', marginLeft: 2 }} />
      </div>
      {!poster && attachment.durationMs ? (
        <text style={{ position: 'absolute', bottom: 8, right: 10, ...TYPE.micro, fontWeight: 600, color: '#ffffff' }}>{formatDuration(attachment.durationMs)}</text>
      ) : null}
    </div>
  )
}

function AudioAttachment({ attachment, message, fromMe }: { attachment: Attachment; message: Message; fromMe: boolean }) {
  const shell = useShell()
  const [playing, setPlaying] = useState(false)
  // playAudio spawns a player; nothing calls back when it exits, so watch it.
  useEffect(() => {
    if (!playing) return
    const timer = setInterval(() => {
      if (!isAudioPlaying()) setPlaying(false)
    }, 400)
    return () => clearInterval(timer)
  }, [playing])
  const toggle = async () => {
    if (playing) {
      stopAudio()
      setPlaying(false)
      return
    }
    const local = attachment.localPath ?? (await shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => undefined))
    if (local && playAudio(local)) setPlaying(true)
  }
  const onFill = fromMe ? C.onAccent : C.text
  return (
    <div
      onClick={() => void toggle()}
      style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10, height: 40, paddingLeft: 4, paddingRight: 14, borderRadius: RADIUS.pill, backgroundColor: fromMe ? C.imessage : C.received, cursor: 'pointer', hover: { opacity: 0.9 } }}
    >
      <div style={{ width: 32, height: 32, borderRadius: 16, backgroundColor: fromMe ? '#ffffff29' : C.ghost, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <svg source={playing ? STOP_GLYPH : PLAY_GLYPH} style={{ width: 13, height: 13, color: onFill, marginLeft: playing ? 0 : 2 }} />
      </div>
      <text style={{ ...TYPE.body, fontWeight: 600, color: onFill }}>{attachment.durationMs ? formatDuration(attachment.durationMs) : 'Audio message'}</text>
    </div>
  )
}

function LinkPreview({ message, fromMe }: { message: Message; fromMe: boolean }) {
  const shell = useShell()
  const [failed, setFailed] = useState(false)
  const preview = message.urlPreview
  const image = preview?.imageAttachmentGuid ? message.attachments.find((item) => item.guid === preview.imageAttachmentGuid) : undefined
  const src = preview?.imagePath ?? image?.localPath
  useEffect(() => {
    if (!image || src || failed) return
    shell.store.attachmentSrc(message.chatGuid, message.guid, image.guid, 'preview.jpg', 'image/jpeg').catch(() => setFailed(true))
  }, [image, src, failed, shell.store, message.chatGuid, message.guid])
  if (!preview) return null
  return (
    <div
      onClick={() => openExternal(preview.url)}
      style={{ width: PREVIEW_WIDTH, borderRadius: RADIUS.bubble, overflow: 'hidden', backgroundColor: C.received, cursor: 'pointer', alignSelf: fromMe ? 'flex-end' : 'flex-start', hover: { opacity: 0.9 } }}
    >
      {src ? <img src={src} objectFit="cover" style={{ width: PREVIEW_WIDTH, height: 150 }} /> : null}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2, paddingLeft: 12, paddingRight: 12, paddingTop: 8, paddingBottom: 9 }}>
        {preview.title ? <text style={{ ...TYPE.body, fontWeight: 600, color: C.text, lineClamp: 2 }}>{preview.title}</text> : null}
        <text style={{ ...TYPE.caption, color: C.secondary }}>{preview.siteName || hostOf(preview.url)}</text>
      </div>
    </div>
  )
}

/**
 * The card Messages shows in place of a bare URL: the rich preview when the
 * server resolved one for this exact link, otherwise a compact host/URL card.
 */
function LinkBubble({ href, message, fromMe, fill, matchesPreview }: { href: string; message: Message; fromMe: boolean; fill: string; matchesPreview: boolean }) {
  if (matchesPreview) return <LinkPreview message={message} fromMe={fromMe} />
  return (
    <div
      onClick={() => openExternal(href)}
      style={{
        width: PREVIEW_WIDTH,
        maxWidth: PREVIEW_WIDTH,
        borderRadius: RADIUS.bubble,
        backgroundColor: fill,
        cursor: 'pointer',
        alignSelf: fromMe ? 'flex-end' : 'flex-start',
        paddingLeft: 12,
        paddingRight: 12,
        paddingTop: 8,
        paddingBottom: 8,
        display: 'flex',
        flexDirection: 'column',
        gap: 2,
        hover: { opacity: 0.9 },
      }}
    >
      <text style={{ ...TYPE.body, fontWeight: 600, color: fromMe ? C.onAccent : C.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{hostOf(href)}</text>
      <text style={{ ...TYPE.caption, color: fromMe ? C.onAccentSoft : C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{href}</text>
    </div>
  )
}

function isAudio(message: Message, attachment: Attachment): boolean {
  return message.isAudio || attachment.mime.startsWith('audio/')
}

function AnyAttachment({ attachment, message, fromMe, maxWidth, handlers }: { attachment: Attachment; message: Message; fromMe: boolean; maxWidth: number; handlers?: BubbleHandlers }) {
  // A stickerFor message is a sticker someone dropped on another message, even
  // when the server did not flag the attachment itself.
  const body = attachment.isSticker || message.stickerFor ? (
    <StickerAttachment attachment={attachment} message={message} />
  ) : isAudio(message, attachment) ? (
    <AudioAttachment attachment={attachment} message={message} fromMe={fromMe} />
  ) : attachment.mime.startsWith('image/') ? (
    <ImageAttachment attachment={attachment} message={message} maxWidth={maxWidth} />
  ) : attachment.mime.startsWith('video/') ? (
    <VideoAttachment attachment={attachment} message={message} fromMe={fromMe} maxWidth={maxWidth} />
  ) : (
    <FileAttachment attachment={attachment} message={message} fromMe={fromMe} />
  )
  if (!handlers) return body
  return (
    <div
      onClick={handlers.onClick}
      onAuxClick={handlers.onAttachmentMenu ? (event) => handlers.onAttachmentMenu?.(attachment, event) : handlers.onMenu}
      style={{ alignSelf: fromMe ? 'flex-end' : 'flex-start', maxWidth: '100%', opacity: handlers.dimmed ? 0.7 : 1 }}
    >
      {body}
    </div>
  )
}

/**
 * A whole message body: the subject, the parts with their attachments where
 * the attributed body put them, and every link pulled out into its own card.
 * Each block paints its own surface, since a sticker, a photo, an audio pill
 * and a link card all sit outside the bubble while text sits inside one.
 */
export function BubbleContent({ message, chat, fromMe, textColor, handlers }: { message: Message; chat: Chat; fromMe: boolean; textColor: string; handlers?: BubbleHandlers }) {
  const fill = fromMe ? (message.service === 'iMessage' ? C.imessage : C.sms) : C.received
  const align = fromMe ? 'flex-end' : 'flex-start'
  const mediaWidth = Math.min(MEDIA_MAX_WIDTH, BUBBLE_MAX_WIDTH - (chat.isGroup && !fromMe ? GROUP_GUTTER : 0))
  const visible = message.attachments.filter((item) => !item.hidden)
  const parts: MessagePart[] = message.parts ?? []
  const placed = new Set(parts.flatMap((part) => (part.kind === 'attachment' ? [part.guid] : [])))
  const loose = visible.filter((item) => !placed.has(item.guid))
  const preview = message.urlPreview
  const hasText = message.text.trim().length > 0
  const emojiOnly = visible.length === 0 && hasText && EMOJI_ONLY.test(message.text.trim())

  const column = { display: 'flex', flexDirection: 'column', alignItems: align, gap: S.x1, minWidth: 0 } as const

  if (emojiOnly) {
    return (
      <div style={column} onClick={handlers?.onClick} onAuxClick={handlers?.onMenu}>
        <text style={{ fontSize: 40, lineHeight: 48, color: C.text }}>{message.text.trim()}</text>
      </div>
    )
  }

  let subjectUsed = false
  const subject = message.subject ? (
    <text style={{ ...TYPE.bubble, fontWeight: 700, color: textColor }}>{message.subject}</text>
  ) : null

  const textBlock = (key: string, body: React.ReactNode) => {
    const withSubject = subject && !subjectUsed
    subjectUsed = subjectUsed || Boolean(subject)
    return (
      <TextBubble key={key} fill={fill} fromMe={fromMe} handlers={handlers}>
        {withSubject ? subject : null}
        {body}
      </TextBubble>
    )
  }

  const blocks: React.ReactNode[] = loose.map((attachment) => (
    <AnyAttachment key={attachment.guid} attachment={attachment} message={message} fromMe={fromMe} maxWidth={mediaWidth} handlers={handlers} />
  ))

  // Tracks whether a link atom already rendered the rich preview inline, so
  // the fallback below does not paint it a second time.
  let previewRendered = false
  const pushLink = (key: string, href: string) => {
    const matches = Boolean(preview && sameUrl(href, preview.url))
    previewRendered = previewRendered || matches
    blocks.push(<LinkBubble key={key} href={href} message={message} fromMe={fromMe} fill={fill} matchesPreview={matches} />)
  }

  if (parts.length > 0) {
    parts.forEach((part, index) => {
      if (part.kind === 'attachment') {
        const attachment = visible.find((item) => item.guid === part.guid)
        if (attachment) {
          blocks.push(<AnyAttachment key={part.guid} attachment={attachment} message={message} fromMe={fromMe} maxWidth={mediaWidth} handlers={handlers} />)
        }
        return
      }
      splitRunsAtLinks(part.runs).forEach((atom, atomIndex) => {
        if (atom.kind === 'link') pushLink(`part-${index}-link-${atomIndex}`, atom.href)
        else blocks.push(textBlock(`part-${index}-text-${atomIndex}`, <RichText runs={atom.runs} fromMe={fromMe} color={textColor} />))
      })
    })
  } else if (hasText) {
    splitTextAtLinks(message.text).forEach((atom, atomIndex) => {
      if (atom.kind === 'link') pushLink(`text-link-${atomIndex}`, atom.href)
      else blocks.push(textBlock(`text-${atomIndex}`, <BubbleText text={atom.value} color={textColor} />))
    })
  }

  if (subject && !subjectUsed) blocks.unshift(textBlock('subject', null))
  if (preview && !previewRendered) blocks.push(<LinkPreview key="preview" message={message} fromMe={fromMe} />)

  return <div style={column}>{blocks}</div>
}
