import { useEffect, useRef, useState } from 'react'
import type { EventPayload } from '@gpuix/react'
import { type Attachment, type Chat, type ImageSize, type Message, type MessagePart, type RichRun, exifOrientation, fitInside, imageSize } from '@messages/core'
import { attachmentsDir, formatBytes, isEmojiOnly } from '@messages/core'
import { isAudioPlaying, openExternal, playAudio, splitLinks, stopAudio } from '@messages/core'
import playSource from 'lucide-static/icons/play.svg' with { type: 'text' }
import stopSource from 'lucide-static/icons/square.svg' with { type: 'text' }
import { BUBBLE_MAX_WIDTH, C, FONT_EMOJI, RADIUS, S, TYPE } from './theme'
import { Icon } from './icons'
import { once, slots } from './limit'
import { useShell } from './context'

export const AUTO_DOWNLOAD_BYTES = 25 * 1024 * 1024
const AUTO_DOWNLOAD_VIDEO_BYTES = 60 * 1024 * 1024
const STICKER_WIDTH = 110
const PREVIEW_WIDTH = 280
const PREVIEW_IMAGE_HEIGHT = 150
/** Cap for an inline photo or video poster; a bubble stays readable past it. */
const MEDIA_MAX_WIDTH = 320
const MEDIA_MAX_HEIGHT = 420
/** Long edge of the copy a bubble paints, twice the box so a HiDPI screen still has pixels to spare. */
const PREVIEW_MAX_EDGE = 2 * MEDIA_MAX_HEIGHT
/** Used to box an image or poster when the server has no width/height for it. */
const FALLBACK_RATIO = 4 / 3
const PLAY_CIRCLE = 44
/** Left for the avatar gutter a group thread puts beside every received row. */
const GROUP_GUTTER = 36
/** GPUI only wraps at whitespace, so an unbroken run past this length pushes the bubble off screen. */
const LONG_TOKEN = /\S{33,}/g
/** Photos in one message share a block of tiles; past four, the last tile counts the rest. */
const GRID_GAP = 2
const GRID_MAX_TILES = 4
const TILE_SIDE = 512
/**
 * ffmpeg runs at once. Every one of them decodes a full photo, and a screen
 * of pictures asks for its cuts all at the same moment; on a four watt laptop
 * that took every core and left nothing to paint with.
 */
const ffmpegSlot = slots(2)
/** One run per output file, however many components are waiting on it. */
const ffmpegOnce = once<string | null>()
/** The tail lobe, the same size whether it is filled with a colour or with the picture's corner. */
const TAIL_WIDTH = 14
const TAIL_HEIGHT = 16
const darwin = process.platform === 'darwin'

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

export function ImageAttachment({ attachment, message, fromMe, maxWidth, tail }: { attachment: Attachment; message: Message; fromMe: boolean; maxWidth?: number; tail?: string }) {
  const shell = useShell()
  const [failed, setFailed] = useState(false)
  const src = attachment.localPath
  const { width, height } = mediaDims(attachment, Math.min(maxWidth ?? MEDIA_MAX_WIDTH, MEDIA_MAX_WIDTH))
  // A cached file comes with the size the server reported, which ignores EXIF
  // orientation, so its header is read once per mount to settle the box.
  const verified = useRef(false)
  const shouldFetch = ((!src && attachment.bytes <= AUTO_DOWNLOAD_BYTES) || (Boolean(src) && !verified.current)) && !failed
  useEffect(() => {
    if (!shouldFetch) return
    if (src) verified.current = true
    shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => setFailed(true))
  }, [shouldFetch, src, shell.store, message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime])
  const oversized = Boolean(attachment.width && attachment.height && Math.max(attachment.width, attachment.height) > PREVIEW_MAX_EDGE)
  const [preview, setPreview] = useState<string | null>(null)
  useEffect(() => {
    if (!src || !oversized) return
    let cancelled = false
    setPreview(null)
    void generatePreview(src, attachment.guid).then((path) => {
      if (!cancelled) setPreview(path ?? src)
    })
    return () => {
      cancelled = true
    }
  }, [src, oversized, attachment.guid])
  // Nothing is painted until the scaled copy is settled: one frame holding the
  // original is enough to pay for its full-size texture.
  const shown = oversized ? preview : src
  if (src) {
    return (
      <TailBox fromMe={fromMe} picture={tail && shown ? shown : undefined}>
        <div
          onClick={() => shell.openLightbox({ chatGuid: message.chatGuid, attachmentGuid: attachment.guid })}
          style={{ width, height, cursor: 'pointer', borderRadius: RADIUS.bubble, overflow: 'hidden', borderWidth: 1, borderColor: '#ffffff1a', backgroundColor: C.received }}
        >
          {shown ? <img src={shown} objectFit="contain" style={{ width, height, borderRadius: RADIUS.bubble }} /> : null}
        </div>
      </TailBox>
    )
  }
  return (
    <TailBox fromMe={fromMe} fill={tail}>
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
    </TailBox>
  )
}

/** `<cache>/<guid>.tile-<aspect>x1-<side>.jpg`: the centre cut of a photo at that aspect, made by ffmpeg the way the video posters are. */
function tileCachePath(guid: string, aspect: number, side: number): string {
  return `${attachmentsDir}/${guid}.tile-${aspect}x1-${side}.jpg`
}

/** `<cache>/<guid>.preview.jpg`: the whole photo at the size a bubble can actually show. */
function previewCachePath(guid: string): string {
  return `${attachmentsDir}/${guid}.preview.jpg`
}

/**
 * `<img>` paints whatever file it is handed, so a 4032x3024 photo in a 320
 * wide bubble still costs 48 MB of RGBA in the texture cache, and a thread of
 * them is enough to take a machine with 8 GB down. Anything past the box gets
 * a scaled copy first, the same bargain the grid tiles already make. The
 * min() keeps a small picture at its own size rather than blowing it up.
 */
async function generatePreview(imagePath: string, guid: string): Promise<string | null> {
  if (imagePath.startsWith('data:')) return null
  const target = previewCachePath(guid)
  if (await Bun.file(target).exists()) return target
  if (!Bun.which('ffmpeg')) return null
  return ffmpegOnce(target, async () => {
    try {
      const head = new Uint8Array(await Bun.file(imagePath).slice(0, 256 * 1024).arrayBuffer())
      const fit = `scale=w='min(iw,${PREVIEW_MAX_EDGE})':h='min(ih,${PREVIEW_MAX_EDGE})':force_original_aspect_ratio=decrease:flags=area`
      return await ffmpegSlot(async () => {
        const proc = Bun.spawn(['ffmpeg', '-y', '-noautorotate', '-i', imagePath, '-vf', `${uprightFilter(exifOrientation(head))}${fit}`, '-frames:v', '1', '-q:v', '4', target], { stdout: 'ignore', stderr: 'ignore' })
        const code = await proc.exited
        return code === 0 && (await Bun.file(target).exists()) ? target : null
      })
    } catch {
      return null
    }
  })
}

/** ffmpeg reads a still the way it is stored, so the EXIF turn goes in by hand ahead of the cut. */
function uprightFilter(orientation: number | null): string {
  switch (orientation) {
    case 2:
      return 'hflip,'
    case 3:
      return 'hflip,vflip,'
    case 4:
      return 'vflip,'
    case 5:
      return 'transpose=0,'
    case 6:
      return 'transpose=1,'
    case 7:
      return 'transpose=3,'
    case 8:
      return 'transpose=2,'
    default:
      return ''
  }
}

/** A tile that fills its box exactly, so nothing has to be clipped at paint time. Null without ffmpeg, or for a data URL. */
export async function generateTile(imagePath: string, guid: string, aspect: number, side = TILE_SIDE): Promise<string | null> {
  if (imagePath.startsWith('data:')) return null
  const target = tileCachePath(guid, aspect, side)
  if (await Bun.file(target).exists()) return target
  if (!Bun.which('ffmpeg')) return null
  return ffmpegOnce(target, async () => {
    try {
      const head = new Uint8Array(await Bun.file(imagePath).slice(0, 256 * 1024).arrayBuffer())
      const cut = `crop=w='min(iw,ih*${aspect})':h='min(ih,iw/${aspect})',scale=${side * aspect}:${side}`
      return await ffmpegSlot(async () => {
        const proc = Bun.spawn(['ffmpeg', '-y', '-noautorotate', '-i', imagePath, '-vf', `${uprightFilter(exifOrientation(head))}${cut}`, '-frames:v', '1', '-q:v', '3', target], { stdout: 'ignore', stderr: 'ignore' })
        const code = await proc.exited
        return code === 0 && (await Bun.file(target).exists()) ? target : null
      })
    } catch {
      return null
    }
  })
}

/** `<cache>/<file>.tail-<side>.jpg`: the bottom corner the tail lobe sits on, cut to the lobe's size. */
function tailCutPath(imagePath: string, side: string): string {
  return `${attachmentsDir}/${imagePath.split('/').pop()}.tail-${side}.jpg`
}

/** The picture's own corner at twice the lobe's size, so the lobe never has to be clipped. Null without ffmpeg, or for a data URL. */
async function generateTailCut(imagePath: string, fromMe: boolean): Promise<string | null> {
  if (imagePath.startsWith('data:') || !Bun.which('ffmpeg')) return null
  const target = tailCutPath(imagePath, fromMe ? 'right' : 'left')
  if (await Bun.file(target).exists()) return target
  return ffmpegOnce(target, async () => {
    try {
      const head = new Uint8Array(await Bun.file(imagePath).slice(0, 256 * 1024).arrayBuffer())
      const cut = `crop=w='iw*0.12':h='ih*0.14':x='${fromMe ? 'iw-out_w' : '0'}':y='ih-out_h',scale=${2 * TAIL_WIDTH}:${2 * TAIL_HEIGHT}`
      return await ffmpegSlot(async () => {
        const proc = Bun.spawn(['ffmpeg', '-y', '-noautorotate', '-i', imagePath, '-vf', `${uprightFilter(exifOrientation(head))}${cut}`, '-frames:v', '1', '-q:v', '3', target], { stdout: 'ignore', stderr: 'ignore' })
        const code = await proc.exited
        return code === 0 && (await Bun.file(target).exists()) ? target : null
      })
    } catch {
      return null
    }
  })
}

interface TileBox {
  index: number
  x: number
  y: number
  width: number
  height: number
}

/** Two squares side by side, a wide tile over two squares, or a two-by-two. */
function gridRows(count: number, total: number): { rows: TileBox[][]; height: number } {
  const half = Math.floor((total - GRID_GAP) / 2)
  const step = half + GRID_GAP
  const square = (index: number, x: number, y: number): TileBox => ({ index, x, y, width: half, height: half })
  if (count === 2) return { rows: [[square(0, 0, 0), square(1, step, 0)]], height: half }
  if (count === 3) {
    const wide = Math.round(total / 2)
    return { rows: [[{ index: 0, x: 0, y: 0, width: total, height: wide }], [square(1, 0, wide + GRID_GAP), square(2, step, wide + GRID_GAP)]], height: wide + GRID_GAP + half }
  }
  return { rows: [[square(0, 0, 0), square(1, step, 0)], [square(2, 0, step), square(3, step, step)]], height: step + half }
}

/** The block's outer corners keep the bubble radius; every corner inside it is tight. */
function tileRadius(box: TileBox, width: number, height: number) {
  const big = RADIUS.bubble
  const small = RADIUS.bubbleTight
  const right = box.x + box.width === width
  const bottom = box.y + box.height === height
  return {
    borderTopLeftRadius: box.x === 0 && box.y === 0 ? big : small,
    borderTopRightRadius: right && box.y === 0 ? big : small,
    borderBottomLeftRadius: box.x === 0 && bottom ? big : small,
    borderBottomRightRadius: right && bottom ? big : small,
  }
}

function PhotoTile({
  attachment,
  message,
  box,
  radius,
  more,
  fromMe,
  tail,
  onMenu,
}: {
  attachment: Attachment
  message: Message
  box: TileBox
  radius: ReturnType<typeof tileRadius>
  more: number
  fromMe: boolean
  tail?: string
  onMenu?: (event: EventPayload) => void
}) {
  const shell = useShell()
  const [tile, setTile] = useState<{ path: string; cut: boolean } | null>(null)
  const [failed, setFailed] = useState(false)
  const src = attachment.localPath
  const aspect = Math.round(box.width / box.height)
  const shouldFetch = !src && attachment.bytes <= AUTO_DOWNLOAD_BYTES && !failed
  useEffect(() => {
    if (!shouldFetch) return
    shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => setFailed(true))
  }, [shouldFetch, shell.store, message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime])
  useEffect(() => {
    if (!src) return
    let cancelled = false
    setTile(null)
    void generateTile(src, attachment.guid, aspect).then((path) => {
      if (!cancelled) setTile({ path: path ?? src, cut: Boolean(path) })
    })
    return () => {
      cancelled = true
    }
  }, [src, attachment.guid, aspect])
  // Nothing is painted until the cut is settled: one frame holding the original
  // is enough to pay for a full-size texture.
  const shown = src ? tile?.path : undefined
  // A cut tile fills its box. Without one the whole photo is fitted, except on macOS, the one renderer that clips a cover fill.
  const fit = tile?.cut || darwin ? 'cover' : 'contain'
  // The tile clips its own picture, so the lobe hangs off this wrapper instead.
  return (
    <div style={{ position: 'relative', display: 'flex', flexShrink: 0 }}>
      {tail ? shown ? <PictureTail fromMe={fromMe} src={shown} /> : <Tail fromMe={fromMe} color={tail} /> : null}
    <div
      testId={`photo-${attachment.guid}`}
      onClick={() => shell.openLightbox({ chatGuid: message.chatGuid, attachmentGuid: attachment.guid })}
      onAuxClick={onMenu}
      style={{ width: box.width, height: box.height, overflow: 'hidden', position: 'relative', backgroundColor: C.received, cursor: 'pointer', flexShrink: 0, ...radius, hover: { opacity: 0.94 } }}
    >
      {shown ? <img src={shown} objectFit={fit} style={{ width: box.width, height: box.height, ...radius }} /> : null}
      {more > 0 ? (
        <div style={{ position: 'absolute', top: 0, left: 0, width: box.width, height: box.height, backgroundColor: '#00000080', display: 'flex', alignItems: 'center', justifyContent: 'center', pointerEvents: 'none' }}>
          <text style={{ fontSize: 22, fontWeight: 600, lineHeight: 28, color: '#ffffff' }}>{`+${more}`}</text>
        </div>
      ) : null}
    </div>
    </div>
  )
}

/** The photos of one message as a single rounded block, each tile opening the lightbox on its own picture. */
function PhotoGrid({ photos, message, fromMe, width, tail, handlers }: { photos: Attachment[]; message: Message; fromMe: boolean; width: number; tail?: string; handlers?: BubbleHandlers }) {
  const shown = photos.slice(0, GRID_MAX_TILES)
  const { rows, height } = gridRows(shown.length, width)
  // The tail belongs to the tile in the grid's bottom outer corner.
  const lastRow = rows[rows.length - 1] ?? []
  const tailTile = fromMe ? lastRow[lastRow.length - 1] : lastRow[0]
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: GRID_GAP, width, alignSelf: fromMe ? 'flex-end' : 'flex-start', flexShrink: 0 }}>
      {rows.map((row, rowIndex) => (
        <div key={rowIndex} style={{ display: 'flex', flexDirection: 'row', gap: GRID_GAP }}>
          {row.map((box) => {
            const attachment = shown[box.index]!
            return (
              <PhotoTile
                key={attachment.guid}
                attachment={attachment}
                message={message}
                box={box}
                radius={tileRadius(box, width, height)}
                more={box.index === shown.length - 1 ? photos.length - shown.length : 0}
                fromMe={fromMe}
                tail={box === tailTile ? tail : undefined}
                onMenu={handlers?.onAttachmentMenu ? (event) => handlers.onAttachmentMenu?.(attachment, event) : handlers?.onMenu}
              />
            )
          })}
        </div>
      ))}
    </div>
  )
}

/**
 * The bubble's fill continues past its rounded corner, then a canvas-coloured
 * quad carves the concave curve back out. Only the block a run ends on has one.
 */
export function Tail({ fromMe, color }: { fromMe: boolean; color: string }) {
  const side = fromMe ? { right: -5 } : { left: -5 }
  const cut = fromMe ? { right: -10 } : { left: -10 }
  return (
    <>
      <div style={{ position: 'absolute', bottom: 0, ...side, width: TAIL_WIDTH, height: TAIL_HEIGHT, backgroundColor: color, ...(fromMe ? { borderBottomLeftRadius: 14 } : { borderBottomRightRadius: 14 }) }} />
      <div style={{ position: 'absolute', bottom: 0, ...cut, width: 10, height: 20, backgroundColor: C.canvas, ...(fromMe ? { borderBottomLeftRadius: 10 } : { borderBottomRightRadius: 10 }) }} />
    </>
  )
}

/**
 * The tail under a photo carries the photo, the way Messages masks the bubble
 * shape over it rather than hanging a coloured nub off the corner. A mask is a
 * rectangle, so an image inside a rounded box keeps its square corners: the
 * picture's corner is cut to the lobe's size on disk and the lobe rounds
 * itself. Nothing is painted until the cut is there.
 */
function PictureTail({ fromMe, src }: { fromMe: boolean; src: string }) {
  const [corner, setCorner] = useState<string | null | undefined>(undefined)
  useEffect(() => {
    let cancelled = false
    void generateTailCut(src, fromMe).then((path) => {
      if (!cancelled) setCorner(path)
    })
    return () => {
      cancelled = true
    }
  }, [src, fromMe])
  const side = fromMe ? { right: -5 } : { left: -5 }
  const cut = fromMe ? { right: -10 } : { left: -10 }
  // Nothing while the cut is being made; a plain nub for a picture that cannot be cut, a data URL or no ffmpeg.
  if (corner === undefined) return null
  if (corner === null) return <Tail fromMe={fromMe} color={C.received} />
  return (
    <>
      <img
        src={corner}
        objectFit="fill"
        style={{ position: 'absolute', bottom: 0, ...side, width: TAIL_WIDTH, height: TAIL_HEIGHT, ...(fromMe ? { borderBottomLeftRadius: 14 } : { borderBottomRightRadius: 14 }) }}
      />
      <div style={{ position: 'absolute', bottom: 0, ...cut, width: 10, height: 20, backgroundColor: C.canvas, ...(fromMe ? { borderBottomLeftRadius: 10 } : { borderBottomRightRadius: 10 }) }} />
    </>
  )
}

/**
 * Holds one block so the tail can hang off its bottom corner rather than off
 * the widest block in the message. The tail paints first: half of it sits
 * inside the block, which then covers that half, so a photo hides the shifted
 * copy of itself and a text bubble hides a fill of its own colour.
 */
function TailBox({ fill, picture, fromMe, children }: { fill?: string; picture?: string; fromMe: boolean; children: React.ReactNode }) {
  return (
    <div style={{ position: 'relative', display: 'flex', flexDirection: 'column', alignItems: fromMe ? 'flex-end' : 'flex-start', maxWidth: '100%', minWidth: 0 }}>
      {picture ? <PictureTail fromMe={fromMe} src={picture} /> : fill ? <Tail fromMe={fromMe} color={fill} /> : null}
      {children}
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
      style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10, paddingLeft: 12, paddingRight: 14, paddingTop: 10, paddingBottom: 10, borderRadius: RADIUS.bubble, backgroundColor: bubbleFill(message, fromMe), cursor: 'pointer', maxWidth: 320 }}
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
 * The poster and the size it was written at. A video shot on a phone stores
 * landscape pixels plus a rotation in its side data, and the size chat.db
 * reports is the stored one; ffmpeg turns the frame upright, so the poster
 * header is the only honest source for the box.
 */
interface Poster extends ImageSize {
  path: string
}

/**
 * Grabs the frame at 0.5s (skipping a black opening frame) and scales it to
 * 640 wide. Silently gives up when ffmpeg is not on PATH or the run fails;
 * the caller falls back to a plain dark box.
 */
async function generatePoster(videoPath: string, guid: string): Promise<Poster | null> {
  const target = posterCachePath(guid)
  const measured = async (): Promise<Poster | null> => {
    const size = await imageSize(target)
    return size ? { path: target, ...size } : null
  }
  if (await Bun.file(target).exists()) return measured()
  if (!Bun.which('ffmpeg')) return null
  const made = await ffmpegOnce(target, async () => {
    try {
      return await ffmpegSlot(async () => {
        const proc = Bun.spawn(['ffmpeg', '-y', '-ss', '0.5', '-i', videoPath, '-frames:v', '1', '-vf', 'scale=640:-1', target], { stdout: 'ignore', stderr: 'ignore' })
        const code = await proc.exited
        return code === 0 && (await Bun.file(target).exists()) ? target : null
      })
    } catch {
      return null
    }
  })
  return made ? measured() : null
}

function VideoAttachment({ attachment, message, fromMe, maxWidth, tail }: { attachment: Attachment; message: Message; fromMe: boolean; maxWidth: number; tail?: string }) {
  const shell = useShell()
  const [failed, setFailed] = useState(false)
  const [poster, setPoster] = useState<Poster | null>(null)
  const posterTried = useRef(false)
  const src = attachment.localPath
  const cap = Math.min(maxWidth, MEDIA_MAX_WIDTH)
  const { width, height } = poster ? fitInside(poster, cap, MEDIA_MAX_HEIGHT) : mediaDims(attachment, cap)
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
      <TailBox fromMe={fromMe} fill={tail}>
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
      </TailBox>
    )
  }

  return (
    <TailBox fromMe={fromMe} fill={poster ? undefined : tail} picture={tail && poster ? poster.path : undefined}>
    <div
      onClick={() => void open()}
      style={{ width, height, borderRadius: RADIUS.bubble, overflow: 'hidden', position: 'relative', cursor: 'pointer', backgroundColor: '#1c1c1e', hover: { opacity: 0.94 } }}
    >
      {poster ? <img src={poster.path} objectFit="contain" style={{ width, height, borderRadius: RADIUS.bubble }} /> : null}
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
    </TailBox>
  )
}

interface Transcript {
  loading: boolean
  text?: string
  error?: string
}

function AudioAttachment({ attachment, message, fromMe, tail }: { attachment: Attachment; message: Message; fromMe: boolean; tail?: string }) {
  const shell = useShell()
  const [playing, setPlaying] = useState(false)
  const [transcript, setTranscript] = useState<Transcript | null>(null)
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
  const transcribe = async () => {
    const assistant = shell.assistant
    if (!assistant || transcript?.loading) return
    const local = attachment.localPath ?? (await shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => undefined))
    if (!local || local.startsWith('data:')) {
      setTranscript({ loading: false, error: 'Could not load the audio file.' })
      return
    }
    setTranscript({ loading: true })
    try {
      const text = await assistant.transcribe(local)
      setTranscript({ loading: false, text })
    } catch (error) {
      setTranscript({ loading: false, error: error instanceof Error ? error.message : String(error) })
    }
  }
  const onFill = fromMe ? C.onAccent : C.text
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4, alignItems: fromMe ? 'flex-end' : 'flex-start', maxWidth: 260 }}>
      <TailBox fill={tail} fromMe={fromMe}>
        <div
          onClick={() => void toggle()}
          style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10, height: 40, paddingLeft: 4, paddingRight: 14, borderRadius: RADIUS.pill, backgroundColor: bubbleFill(message, fromMe), cursor: 'pointer', hover: { opacity: 0.9 } }}
        >
          <div style={{ width: 32, height: 32, borderRadius: 16, backgroundColor: fromMe ? '#ffffff29' : C.ghost, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
            <svg source={playing ? STOP_GLYPH : PLAY_GLYPH} style={{ width: 13, height: 13, color: onFill, marginLeft: playing ? 0 : 2 }} />
          </div>
          <text style={{ ...TYPE.body, fontWeight: 600, color: onFill }}>{attachment.durationMs ? formatDuration(attachment.durationMs) : 'Audio message'}</text>
        </div>
      </TailBox>
      {shell.assistant && !transcript?.text ? (
        <div testId="transcribe" onClick={() => void transcribe()} style={{ cursor: transcript?.loading ? 'default' : 'pointer', hover: transcript?.loading ? undefined : { opacity: 0.8 } }}>
          <text style={{ ...TYPE.caption, color: transcript?.error ? C.danger : C.accent }}>{transcript?.loading ? 'Transcribing…' : (transcript?.error ?? 'Transcribe')}</text>
        </div>
      ) : null}
      {transcript?.text ? (
        <text testId="transcript" style={{ ...TYPE.caption, color: fromMe ? C.onAccentSoft : C.secondary }}>
          {transcript.text}
        </text>
      ) : null}
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
  // The list measures the row once, so the picture's box is there from the first paint, filled or not.
  const hasPicture = Boolean(src || (image && !failed))
  return (
    <div
      onClick={() => openExternal(preview.url)}
      style={{ width: PREVIEW_WIDTH, borderRadius: RADIUS.bubble, overflow: 'hidden', backgroundColor: C.received, cursor: 'pointer', alignSelf: fromMe ? 'flex-end' : 'flex-start', hover: { opacity: 0.9 } }}
    >
      {hasPicture ? (
        <div style={{ width: PREVIEW_WIDTH, height: PREVIEW_IMAGE_HEIGHT, backgroundColor: C.raised, borderTopLeftRadius: RADIUS.bubble, borderTopRightRadius: RADIUS.bubble, overflow: 'hidden' }}>
          {src ? <img src={src} objectFit="cover" style={{ width: PREVIEW_WIDTH, height: PREVIEW_IMAGE_HEIGHT, borderTopLeftRadius: RADIUS.bubble, borderTopRightRadius: RADIUS.bubble }} /> : null}
        </div>
      ) : null}
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

function AnyAttachment({ attachment, message, fromMe, maxWidth, tail, handlers }: { attachment: Attachment; message: Message; fromMe: boolean; maxWidth: number; tail?: string; handlers?: BubbleHandlers }) {
  // A stickerFor message is a sticker someone dropped on another message, even
  // when the server did not flag the attachment itself.
  const sticker = Boolean(attachment.isSticker || message.stickerFor)
  const audio = !sticker && isAudio(message, attachment)
  // A picture, a video and the audio pill hang their own tail: the first two
  // carry the picture into it, and the pill is only part of its block, with
  // the transcript sitting below it.
  const own = audio || (!sticker && (attachment.mime.startsWith('image/') || attachment.mime.startsWith('video/')))
  const body = sticker ? (
    <StickerAttachment attachment={attachment} message={message} />
  ) : audio ? (
    <AudioAttachment attachment={attachment} message={message} fromMe={fromMe} tail={tail} />
  ) : attachment.mime.startsWith('image/') ? (
    <ImageAttachment attachment={attachment} message={message} fromMe={fromMe} maxWidth={maxWidth} tail={tail} />
  ) : attachment.mime.startsWith('video/') ? (
    <VideoAttachment attachment={attachment} message={message} fromMe={fromMe} maxWidth={maxWidth} tail={tail} />
  ) : (
    <FileAttachment attachment={attachment} message={message} fromMe={fromMe} />
  )
  const held = handlers ? (
    <div
      onClick={handlers.onClick}
      onAuxClick={handlers.onAttachmentMenu ? (event) => handlers.onAttachmentMenu?.(attachment, event) : handlers.onMenu}
      style={{ alignSelf: fromMe ? 'flex-end' : 'flex-start', maxWidth: '100%' }}
    >
      {body}
    </div>
  ) : (
    body
  )
  return <TailBox fill={own ? undefined : tail} fromMe={fromMe}>{held}</TailBox>
}

type Block =
  | { key: string; kind: 'photos'; photos: Attachment[] }
  | { key: string; kind: 'attachment'; attachment: Attachment }
  | { key: string; kind: 'link'; href: string; matches: boolean }
  | { key: string; kind: 'text'; runs?: RichRun[]; value?: string }
  | { key: string; kind: 'preview' }

function emojiOnlyMessage(message: Message): boolean {
  return !message.attachments.some((item) => !item.hidden) && isEmojiOnly(message.text)
}

/**
 * The blocks a message body paints, in order: the photo grid, the attachments
 * the attributed body did not place, then the parts with every link split out
 * into its own card, and the rich preview last when no link atom carried it.
 * Pure, because the tail hangs off whichever block ends up last.
 */
function blocksOf(message: Message): Block[] {
  if (emojiOnlyMessage(message)) return []
  const visible = message.attachments.filter((item) => !item.hidden)
  const parts: MessagePart[] = message.parts ?? []
  const placed = new Set(parts.flatMap((part) => (part.kind === 'attachment' ? [part.guid] : [])))
  // Two or more photos share one block, wherever the attributed body put them.
  const photos = message.stickerFor ? [] : visible.filter((item) => item.mime.startsWith('image/') && !item.isSticker)
  const grid = photos.length >= 2 ? new Set(photos.map((item) => item.guid)) : null
  const preview = message.urlPreview

  const blocks: Block[] = grid ? [{ key: 'photos', kind: 'photos', photos }] : []
  for (const attachment of visible) {
    if (placed.has(attachment.guid) || grid?.has(attachment.guid)) continue
    blocks.push({ key: attachment.guid, kind: 'attachment', attachment })
  }

  // Tracks whether a link atom already took the rich preview, so the fallback
  // below does not paint it a second time.
  let previewRendered = false
  const pushLink = (key: string, href: string) => {
    const matches = Boolean(preview && sameUrl(href, preview.url))
    previewRendered = previewRendered || matches
    blocks.push({ key, kind: 'link', href, matches })
  }

  if (parts.length > 0) {
    parts.forEach((part, index) => {
      if (part.kind === 'attachment') {
        const attachment = grid?.has(part.guid) ? undefined : visible.find((item) => item.guid === part.guid)
        if (attachment) blocks.push({ key: part.guid, kind: 'attachment', attachment })
        return
      }
      splitRunsAtLinks(part.runs).forEach((atom, atomIndex) => {
        if (atom.kind === 'link') pushLink(`part-${index}-link-${atomIndex}`, atom.href)
        else blocks.push({ key: `part-${index}-text-${atomIndex}`, kind: 'text', runs: atom.runs })
      })
    })
  } else if (message.text.trim().length > 0) {
    splitTextAtLinks(message.text).forEach((atom, atomIndex) => {
      if (atom.kind === 'link') pushLink(`text-link-${atomIndex}`, atom.href)
      else blocks.push({ key: `text-${atomIndex}`, kind: 'text', value: atom.value })
    })
  }

  if (message.subject && !blocks.some((block) => block.kind === 'text')) blocks.unshift({ key: 'subject', kind: 'text' })
  if (preview && !previewRendered) blocks.push({ key: 'preview', kind: 'preview' })
  return blocks
}

/**
 * The colour a block paints its own surface with, which is what its tail has
 * to match. A link card is its own surface: the rich preview paints the
 * received grey whoever sent it, the bare host card takes the bubble fill. A
 * sticker is a transparent PNG dropped on the thread, so it carries no tail.
 */
function blockFill(block: Block, message: Message, fromMe: boolean): string | undefined {
  if (block.kind === 'preview') return C.received
  if (block.kind === 'link' && block.matches) return C.received
  if (block.kind === 'attachment' && (block.attachment.isSticker || message.stickerFor)) return undefined
  return bubbleFill(message, fromMe)
}

function bubbleFill(message: Message, fromMe: boolean): string {
  return fromMe ? (message.service === 'iMessage' ? C.imessage : C.sms) : C.received
}

/**
 * A whole message body: the subject, the parts with their attachments where
 * the attributed body put them, and every link pulled out into its own card.
 * Each block paints its own surface, since a sticker, a photo, an audio pill
 * and a link card all sit outside the bubble while text sits inside one.
 */
export function BubbleContent({ message, chat, fromMe, textColor, tail, handlers }: { message: Message; chat: Chat; fromMe: boolean; textColor: string; tail?: boolean; handlers?: BubbleHandlers }) {
  const fill = bubbleFill(message, fromMe)
  const align = fromMe ? 'flex-end' : 'flex-start'
  const mediaWidth = Math.min(MEDIA_MAX_WIDTH, BUBBLE_MAX_WIDTH - (chat.isGroup && !fromMe ? GROUP_GUTTER : 0))

  const column = { display: 'flex', flexDirection: 'column', alignItems: align, gap: S.x1, minWidth: 0 } as const

  if (emojiOnlyMessage(message)) {
    return (
      <div style={column} onClick={handlers?.onClick} onAuxClick={handlers?.onMenu}>
        <text style={{ fontFamily: FONT_EMOJI, fontSize: 40, lineHeight: 48, color: C.text }}>{message.text.trim()}</text>
      </div>
    )
  }

  let subjectUsed = false
  const subject = message.subject ? (
    <text style={{ ...TYPE.bubble, fontWeight: 700, color: textColor }}>{message.subject}</text>
  ) : null

  const blocks = blocksOf(message)
  const last = blocks.at(-1)?.key
  const tailOf = (block: Block) => (tail && block.key === last ? blockFill(block, message, fromMe) : undefined)

  const render = (block: Block) => {
    switch (block.kind) {
      case 'photos':
        return <PhotoGrid key={block.key} photos={block.photos} message={message} fromMe={fromMe} width={mediaWidth} tail={tailOf(block)} handlers={handlers} />
      case 'attachment':
        return <AnyAttachment key={block.key} attachment={block.attachment} message={message} fromMe={fromMe} maxWidth={mediaWidth} tail={tailOf(block)} handlers={handlers} />
      case 'link':
        return (
          <TailBox key={block.key} fill={tailOf(block)} fromMe={fromMe}>
            <LinkBubble href={block.href} message={message} fromMe={fromMe} fill={fill} matchesPreview={block.matches} />
          </TailBox>
        )
      case 'preview':
        return (
          <TailBox key={block.key} fill={tailOf(block)} fromMe={fromMe}>
            <LinkPreview message={message} fromMe={fromMe} />
          </TailBox>
        )
      case 'text': {
        const withSubject = subject && !subjectUsed
        subjectUsed = subjectUsed || Boolean(subject)
        return (
          <TailBox key={block.key} fill={tailOf(block)} fromMe={fromMe}>
            <TextBubble fill={fill} fromMe={fromMe} handlers={handlers}>
              {withSubject ? subject : null}
              {block.runs ? <RichText runs={block.runs} fromMe={fromMe} color={textColor} /> : null}
              {block.value !== undefined ? <BubbleText text={block.value} color={textColor} /> : null}
            </TextBubble>
          </TailBox>
        )
      }
    }
  }

  return <div style={column}>{blocks.map(render)}</div>
}
