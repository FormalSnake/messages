import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { deliveryState, handleName, tapbackGlyph, type Attachment, type Capabilities, type Chat, type Message, type Tapback } from '@messages/core'
import { formatBytes, formatSeparator, formatTime, needsSeparator } from '@messages/core'
import { useAppState } from './use-app-state'
import { copyText } from '@messages/core'
import { openExternal, splitLinks } from '@messages/core'
import { BUBBLE_MAX_WIDTH, C, RADIUS, TYPE } from './theme'
import { Icon } from './icons'
import { Avatar } from './primitives'
import { useShell, type MenuItem } from './context'

type Position = 'single' | 'first' | 'middle' | 'last'

type Row =
  | { kind: 'separator'; key: string; label: string }
  | { kind: 'event'; key: string; text: string }
  | { kind: 'message'; key: string; message: Message; position: Position; showSender: boolean; receipt: string | null }
  | { kind: 'typing'; key: string }
  | { kind: 'loading'; key: string }

const RUN_GAP = 60_000
const EDIT_WINDOW = 15 * 60_000
const UNSEND_WINDOW = 2 * 60_000
const AUTO_DOWNLOAD_BYTES = 5 * 1024 * 1024
const EMOJI_ONLY = /^(?:\p{Extended_Pictographic}️?(?:‍\p{Extended_Pictographic}️?)*\s*){1,3}$/u

function sameAuthor(a: Message, b: Message): boolean {
  if (a.fromMe !== b.fromMe) return false
  if (a.fromMe) return true
  return a.sender?.address === b.sender?.address
}

function eventText(message: Message, chat: Chat): string | null {
  const who = message.fromMe ? 'You' : message.sender ? handleName(message.sender) : 'Someone'
  if (message.dateRetracted) return `${who} unsent a message.`
  const event = message.groupEvent
  if (!event) return null
  switch (event.kind) {
    case 'rename':
      return `${who} named the conversation “${event.title}”.`
    case 'join':
      return `${who} added ${event.who ? handleName(event.who) : 'someone'}.`
    case 'leave':
      return `${event.who ? handleName(event.who) : who} left the conversation.`
    case 'photo':
      return `${who} changed the group photo.`
  }
  return chat ? null : null
}

export function buildRows(messages: Message[], chat: Chat, typing: boolean, loading: boolean): Row[] {
  const rows: Row[] = []
  if (loading) rows.push({ kind: 'loading', key: 'loading' })
  let lastMineIndex = -1
  let lastReadIndex = -1
  messages.forEach((message, index) => {
    if (message.fromMe && !message.error && !message.groupEvent && !message.dateRetracted) {
      lastMineIndex = index
      if (message.dateRead) lastReadIndex = index
    }
  })
  let previousDate: number | undefined
  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index]!
    const key = message.tempGuid ?? message.guid
    if (needsSeparator(previousDate, message.date)) rows.push({ kind: 'separator', key: `sep-${key}`, label: formatSeparator(message.date) })
    previousDate = message.date
    const asEvent = eventText(message, chat)
    if (asEvent) {
      rows.push({ kind: 'event', key, text: asEvent })
      continue
    }
    const previous = messages[index - 1]
    const next = messages[index + 1]
    const joinsPrevious = Boolean(previous && sameAuthor(previous, message) && !eventText(previous, chat) && message.date - previous.date < RUN_GAP && !needsSeparator(previous.date, message.date))
    const joinsNext = Boolean(next && sameAuthor(message, next) && !eventText(next, chat) && next.date - message.date < RUN_GAP && !needsSeparator(message.date, next.date))
    const position: Position = joinsPrevious && joinsNext ? 'middle' : joinsPrevious ? 'last' : joinsNext ? 'first' : 'single'
    let receipt: string | null = null
    if (message.fromMe) {
      const state = deliveryState(message)
      if (state === 'failed') receipt = 'Not delivered'
      else if (state === 'sending') receipt = 'Sending…'
      else if (index === lastReadIndex && message.dateRead) receipt = `Read ${formatTime(message.dateRead)}`
      else if (index === lastMineIndex && lastReadIndex < index) receipt = state === 'delivered' ? 'Delivered' : message.service === 'iMessage' ? 'Sent' : 'Sent as text message'
    }
    rows.push({ kind: 'message', key, message, position, showSender: chat.isGroup && !message.fromMe && (position === 'first' || position === 'single'), receipt })
  }
  if (typing) rows.push({ kind: 'typing', key: 'typing' })
  return rows
}

function bubbleRadius(fromMe: boolean, position: Position): Partial<Record<'borderTopLeftRadius' | 'borderTopRightRadius' | 'borderBottomLeftRadius' | 'borderBottomRightRadius', number>> {
  const big = RADIUS.bubble
  const small = 5
  const top = position === 'middle' || position === 'last' ? small : big
  const bottom = position === 'middle' || position === 'first' ? small : big
  return fromMe
    ? { borderTopLeftRadius: big, borderBottomLeftRadius: big, borderTopRightRadius: top, borderBottomRightRadius: bottom }
    : { borderTopRightRadius: big, borderBottomRightRadius: big, borderTopLeftRadius: top, borderBottomLeftRadius: bottom }
}

function Tail({ fromMe, color }: { fromMe: boolean; color: string }) {
  const side = fromMe ? { right: -5 } : { left: -5 }
  const cut = fromMe ? { right: -10 } : { left: -10 }
  return (
    <>
      <div style={{ position: 'absolute', bottom: 0, ...side, width: 14, height: 16, backgroundColor: color, ...(fromMe ? { borderBottomLeftRadius: 14 } : { borderBottomRightRadius: 14 }) }} />
      <div style={{ position: 'absolute', bottom: 0, ...cut, width: 10, height: 20, backgroundColor: C.canvas, ...(fromMe ? { borderBottomLeftRadius: 10 } : { borderBottomRightRadius: 10 }) }} />
    </>
  )
}

function Tapbacks({ tapbacks, fromMe }: { tapbacks: Tapback[]; fromMe: boolean }) {
  const groups = new Map<string, { glyph: string; count: number; mine: boolean }>()
  for (const tapback of tapbacks) {
    const glyph = tapbackGlyph(tapback)
    const entry = groups.get(glyph) ?? { glyph, count: 0, mine: false }
    entry.count += 1
    entry.mine = entry.mine || tapback.fromMe
    groups.set(glyph, entry)
  }
  const items = [...groups.values()].slice(0, 3)
  return (
    <div style={{ position: 'absolute', top: -13, ...(fromMe ? { left: -6 } : { right: -6 }), display: 'flex', flexDirection: fromMe ? 'row-reverse' : 'row', gap: -6 }}>
      {items.map((item) => (
        <div
          key={item.glyph}
          style={{
            display: 'flex',
            flexDirection: 'row',
            alignItems: 'center',
            gap: 2,
            height: 26,
            paddingLeft: item.count > 1 ? 8 : 6,
            paddingRight: item.count > 1 ? 8 : 6,
            borderRadius: 13,
            backgroundColor: item.mine ? C.tapbackMine : C.tapback,
            borderWidth: 2,
            borderColor: C.canvas,
            userSelect: 'none',
          }}
        >
          <text style={{ fontSize: 12, lineHeight: 16, color: C.text }}>{item.glyph}</text>
          {item.count > 1 ? <text style={{ ...TYPE.micro, fontWeight: 600, color: C.onAccent }}>{String(item.count)}</text> : null}
        </div>
      ))}
    </div>
  )
}

function ImageAttachment({ attachment, message }: { attachment: Attachment; message: Message }) {
  const shell = useShell()
  const [failed, setFailed] = useState(false)
  const src = attachment.localPath
  const width = Math.min(BUBBLE_MAX_WIDTH - 160, attachment.width ?? 280)
  const height = attachment.width && attachment.height ? Math.round((width * attachment.height) / attachment.width) : Math.round(width * 0.66)
  const shouldFetch = !src && attachment.bytes <= AUTO_DOWNLOAD_BYTES && !failed
  useEffect(() => {
    if (!shouldFetch) return
    shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name).catch(() => setFailed(true))
  }, [shouldFetch, shell.store, message.chatGuid, message.guid, attachment.guid, attachment.name])
  if (src) {
    return (
      <div onClick={() => (src.startsWith('data:') ? undefined : openExternal(src))} style={{ cursor: src.startsWith('data:') ? 'default' : 'pointer', borderRadius: RADIUS.bubble, overflow: 'hidden', borderWidth: 1, borderColor: '#ffffff1a' }}>
        <img src={src} objectFit="cover" style={{ width, height: Math.min(height, 420) }} />
      </div>
    )
  }
  return (
    <div
      onClick={() => {
        setFailed(false)
        shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name).catch(() => setFailed(true))
      }}
      style={{ width, height: Math.min(height, 420), borderRadius: RADIUS.bubble, backgroundColor: C.received, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 6, cursor: 'pointer' }}
    >
      <Icon name="image" size={22} color={C.secondary} />
      <text style={{ ...TYPE.caption, color: C.secondary }}>{failed ? 'Could not load. Click to retry.' : shouldFetch ? 'Loading…' : `Click to download (${formatBytes(attachment.bytes)})`}</text>
    </div>
  )
}

function FileAttachment({ attachment, message, fromMe }: { attachment: Attachment; message: Message; fromMe: boolean }) {
  const shell = useShell()
  const open = async () => {
    const local = attachment.localPath ?? (await shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name).catch(() => undefined))
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

function BubbleText({ text, color }: { text: string; color: string }) {
  const lines = text.split('\n')
  return (
    <div style={{ display: 'flex', flexDirection: 'column' }}>
      {lines.map((line, index) => {
        const segments = splitLinks(line)
        if (segments.length === 1 && segments[0]?.kind === 'text') {
          return (
            <text key={index} style={{ ...TYPE.bubble, color }}>
              {line.length ? line : ' '}
            </text>
          )
        }
        return (
          <div key={index} style={{ display: 'flex', flexDirection: 'row', flexWrap: 'wrap' }}>
            {segments.map((segment, segmentIndex) =>
              segment.kind === 'link' ? (
                <text key={segmentIndex} onClick={() => openExternal(segment.href)} style={{ ...TYPE.bubble, color, cursor: 'pointer', hover: { opacity: 0.8 } }}>
                  {segment.value}
                </text>
              ) : (
                <text key={segmentIndex} style={{ ...TYPE.bubble, color }}>
                  {segment.value}
                </text>
              ),
            )}
          </div>
        )
      })}
    </div>
  )
}

function ReplyQuote({ original, fromMe }: { original: Message | undefined; fromMe: boolean }) {
  if (!original) return null
  const who = original.fromMe ? 'You' : original.sender ? handleName(original.sender) : ''
  const body = original.text || (original.attachments.length ? 'Attachment' : '')
  return (
    <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'stretch', gap: 8, marginBottom: 4, maxWidth: BUBBLE_MAX_WIDTH - 40, alignSelf: fromMe ? 'flex-end' : 'flex-start' }}>
      <div style={{ width: 2, borderRadius: 1, backgroundColor: fromMe ? C.imessage : C.tertiary, flexShrink: 0 }} />
      <div style={{ display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <text style={{ ...TYPE.micro, fontWeight: 600, color: C.secondary }}>{who}</text>
        <text style={{ ...TYPE.caption, color: C.secondary, lineClamp: 2 }}>{body}</text>
      </div>
    </div>
  )
}

function messageMenu(message: Message, chat: Chat, capabilities: Capabilities, shell: ReturnType<typeof useShell>): MenuItem[] {
  const { store } = shell
  const age = Date.now() - message.date
  const items: MenuItem[] = []
  const failed = deliveryState(message) === 'failed'
  if (capabilities.reactions && !failed) items.push({ kind: 'tapbacks', chatGuid: chat.guid, messageGuid: message.guid })
  if (failed) items.push({ label: 'Try again', icon: 'refresh', onSelect: () => void store.retry(chat.guid, message.guid) })
  if (capabilities.replies && !failed) items.push({ label: 'Reply', icon: 'reply', onSelect: () => store.setReplyingTo(chat.guid, message.guid) })
  if (message.text) items.push({ label: 'Copy', icon: 'copy', onSelect: () => void copyText(message.text) })
  for (const segment of splitLinks(message.text)) {
    if (segment.kind === 'link') items.push({ label: 'Open link', onSelect: () => openExternal(segment.href) })
  }
  const image = message.attachments.find((item) => item.localPath && !item.localPath.startsWith('data:'))
  if (image?.localPath) items.push({ label: 'Open attachment', icon: 'image', onSelect: () => openExternal(image.localPath!) })
  if (message.fromMe && !failed && message.service === 'iMessage' && (capabilities.edit || capabilities.unsend)) {
    items.push({ kind: 'separator' })
    if (capabilities.edit) items.push({ label: 'Edit', icon: 'edit', disabled: age > EDIT_WINDOW || message.attachments.length > 0, onSelect: () => store.setEditing(chat.guid, message.guid) })
    if (capabilities.unsend) items.push({ label: 'Undo send', icon: 'trash', danger: true, disabled: age > UNSEND_WINDOW, onSelect: () => void store.unsend(chat.guid, message.guid) })
  }
  return items
}

const MessageRow = memo(function MessageRow({ message, chat, position, showSender, receipt, capabilities, original }: { message: Message; chat: Chat; position: Position; showSender: boolean; receipt: string | null; capabilities: Capabilities; original?: Message }) {
  const shell = useShell()
  const [hovered, setHovered] = useState(false)
  const fromMe = message.fromMe
  const mine = message.service === 'iMessage' ? C.imessage : C.sms
  const fill = fromMe ? mine : C.received
  const textColor = fromMe ? C.onAccent : C.receivedText
  const emojiOnly = !message.attachments.length && EMOJI_ONLY.test(message.text.trim())
  const state = deliveryState(message)
  const showTail = position === 'last' || position === 'single'
  const showAvatar = chat.isGroup && !fromMe
  const images = message.attachments.filter((item) => item.mime.startsWith('image/') && !item.isSticker)
  const files = message.attachments.filter((item) => !item.mime.startsWith('image/') || item.isSticker)
  const hasText = message.text.trim().length > 0

  return (
    <div
      testId={`message-${message.guid}`}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onAuxClick={(event) => {
        if (event.isRightClick) shell.openMenu({ x: event.x ?? 0, y: event.y ?? 0, items: messageMenu(message, chat, capabilities, shell) })
      }}
      style={{ display: 'flex', flexDirection: 'column', paddingLeft: 16, paddingRight: 16, paddingTop: position === 'first' || position === 'single' ? 6 : 1, paddingBottom: 1 }}
    >
      {showSender && message.sender ? <text style={{ ...TYPE.micro, color: C.secondary, paddingLeft: showAvatar ? 36 : 12, paddingBottom: 2 }}>{handleName(message.sender)}</text> : null}
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-end', justifyContent: fromMe ? 'flex-end' : 'flex-start', gap: 8 }}>
        {showAvatar ? <div style={{ width: 28, flexShrink: 0, display: 'flex', alignItems: 'flex-end' }}>{showTail ? <Avatar handle={message.sender} size={28} /> : null}</div> : null}
        {fromMe && hovered ? <text style={{ ...TYPE.micro, color: C.tertiary, paddingBottom: 6, userSelect: 'none' }}>{formatTime(message.date)}</text> : null}
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: fromMe ? 'flex-end' : 'flex-start', maxWidth: BUBBLE_MAX_WIDTH, minWidth: 0 }}>
          {message.replyTo ? <ReplyQuote original={original} fromMe={fromMe} /> : null}
          {images.map((attachment) => (
            <div key={attachment.guid} style={{ position: 'relative', marginBottom: hasText || files.length ? 4 : 0 }}>
              <ImageAttachment attachment={attachment} message={message} />
            </div>
          ))}
          {files.map((attachment) => (
            <div key={attachment.guid} style={{ marginBottom: hasText ? 4 : 0 }}>
              <FileAttachment attachment={attachment} message={message} fromMe={fromMe} />
            </div>
          ))}
          {hasText ? (
            emojiOnly ? (
              <div style={{ position: 'relative', paddingTop: message.tapbacks.length ? 10 : 0 }}>
                <text style={{ fontSize: 40, lineHeight: 48, color: C.text }}>{message.text.trim()}</text>
                {message.tapbacks.length ? <Tapbacks tapbacks={message.tapbacks} fromMe={fromMe} /> : null}
              </div>
            ) : (
              <div style={{ position: 'relative', marginTop: message.tapbacks.length && !message.replyTo ? 10 : 0 }}>
                <div style={{ paddingLeft: 12, paddingRight: 12, paddingTop: 7, paddingBottom: 7, backgroundColor: fill, ...bubbleRadius(fromMe, position), opacity: state === 'sending' ? 0.7 : 1 }}>
                  {message.subject ? <text style={{ ...TYPE.bubble, fontWeight: 700, color: textColor }}>{message.subject}</text> : null}
                  <BubbleText text={message.text} color={textColor} />
                </div>
                {showTail ? <Tail fromMe={fromMe} color={fill} /> : null}
                {message.tapbacks.length ? <Tapbacks tapbacks={message.tapbacks} fromMe={fromMe} /> : null}
              </div>
            )
          ) : message.tapbacks.length && (images.length || files.length) ? (
            <div style={{ position: 'relative', height: 0 }}>
              <Tapbacks tapbacks={message.tapbacks} fromMe={fromMe} />
            </div>
          ) : null}
          {message.dateEdited ? <text style={{ ...TYPE.micro, color: C.tertiary, paddingTop: 2, paddingLeft: 4, paddingRight: 4 }}>Edited</text> : null}
          {message.effect ? <text style={{ ...TYPE.micro, color: C.tertiary, paddingTop: 2, paddingLeft: 4, paddingRight: 4 }}>{`Sent with ${effectName(message.effect)}`}</text> : null}
          {receipt ? (
            <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 4, paddingTop: 3, paddingRight: 4 }}>
              {state === 'failed' ? <Icon name="alert" size={12} color={C.danger} /> : null}
              <text testId={state === 'failed' ? `failed-${message.guid}` : undefined} style={{ ...TYPE.micro, fontWeight: 600, color: state === 'failed' ? C.danger : C.tertiary }}>{receipt}</text>
            </div>
          ) : null}
        </div>
        {!fromMe && hovered ? <text style={{ ...TYPE.micro, color: C.tertiary, paddingBottom: 6, userSelect: 'none' }}>{formatTime(message.date)}</text> : null}
      </div>
    </div>
  )
})

const EFFECT_NAMES: Record<string, string> = {
  'com.apple.MobileSMS.expressivesend.impact': 'Slam',
  'com.apple.MobileSMS.expressivesend.loud': 'Loud',
  'com.apple.MobileSMS.expressivesend.gentle': 'Gentle',
  'com.apple.MobileSMS.expressivesend.invisibleink': 'Invisible Ink',
  'com.apple.messages.effect.CKEchoEffect': 'Echo',
  'com.apple.messages.effect.CKSpotlightEffect': 'Spotlight',
  'com.apple.messages.effect.CKHappyBirthdayEffect': 'Balloons',
  'com.apple.messages.effect.CKConfettiEffect': 'Confetti',
  'com.apple.messages.effect.CKHeartEffect': 'Love',
  'com.apple.messages.effect.CKLasersEffect': 'Lasers',
  'com.apple.messages.effect.CKFireworksEffect': 'Fireworks',
  'com.apple.messages.effect.CKShootingStarEffect': 'Shooting Star',
  'com.apple.messages.effect.CKSparklesEffect': 'Celebration',
}

export function effectName(id: string): string {
  return EFFECT_NAMES[id] ?? id.split('.').pop() ?? id
}

function TypingRow() {
  return (
    <div style={{ display: 'flex', flexDirection: 'row', paddingLeft: 16, paddingTop: 6, paddingBottom: 2 }}>
      <div style={{ position: 'relative' }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 4, height: 34, paddingLeft: 14, paddingRight: 14, borderRadius: RADIUS.bubble, backgroundColor: C.received }}>
          {[0.35, 0.6, 1].map((opacity, index) => (
            <div key={index} style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: C.secondary, opacity }} />
          ))}
        </div>
        <Tail fromMe={false} color={C.received} />
      </div>
    </div>
  )
}

export function Thread({ chat }: { chat: Chat }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const messages = state.messages[chat.guid] ?? []
  const typing = Boolean(state.typing[chat.guid])
  const loading = Boolean(state.loading[chat.guid]) && messages.length > 0
  const rows = useMemo(() => buildRows(messages, chat, typing, loading), [messages, chat, typing, loading])
  const byGuid = useMemo(() => new Map(messages.map((message) => [message.guid, message])), [messages])
  const requested = useRef(false)

  useEffect(() => {
    requested.current = false
  }, [chat.guid, messages.length])

  if (messages.length === 0 && !state.loading[chat.guid]) {
    return (
      <div testId="thread" style={{ flexGrow: 1, minHeight: 0, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 8 }}>
        <Avatar chat={chat} size={72} />
        <text style={{ ...TYPE.caption, color: C.secondary }}>{chat.service === 'iMessage' ? 'iMessage' : 'Text message'}</text>
        <text style={{ ...TYPE.body, color: C.secondary }}>Send your first message.</text>
      </div>
    )
  }

  return (
    <div testId="thread" style={{ flexGrow: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
    <virtual-list
      key={chat.guid}
      alignment="bottom"
      followTail
      estimatedItemHeight={44}
      overdraw={700}
      onVisibleRange={(event) => {
        if ((event.startIndex ?? 99) > 3 || requested.current) return
        if (!state.hasOlder[chat.guid] || state.loading[chat.guid]) return
        requested.current = true
        void shell.store.loadOlder(chat.guid)
      }}
      style={{ flexGrow: 1, minHeight: 0, width: '100%' }}
    >
      {rows.map((row) => {
        switch (row.kind) {
          case 'separator':
            return (
              <div key={row.key} style={{ display: 'flex', alignItems: 'center', paddingTop: 16, paddingBottom: 6, userSelect: 'none' }}>
                <text style={{ ...TYPE.micro, fontWeight: 600, color: C.tertiary }}>{row.label}</text>
              </div>
            )
          case 'event':
            return (
              <div key={row.key} style={{ display: 'flex', alignItems: 'center', paddingTop: 8, paddingBottom: 4, paddingLeft: 40, paddingRight: 40 }}>
                <text style={{ ...TYPE.micro, color: C.tertiary, textAlign: 'center' }}>{row.text}</text>
              </div>
            )
          case 'loading':
            return (
              <div key={row.key} style={{ display: 'flex', alignItems: 'center', paddingTop: 12, paddingBottom: 4 }}>
                <text style={{ ...TYPE.micro, color: C.tertiary }}>Loading earlier messages…</text>
              </div>
            )
          case 'typing':
            return <TypingRow key={row.key} />
          case 'message':
            return (
              <MessageRow
                key={row.key}
                message={row.message}
                chat={chat}
                position={row.position}
                showSender={row.showSender}
                receipt={row.receipt}
                capabilities={state.capabilities}
                original={row.message.replyTo ? byGuid.get(row.message.replyTo) : undefined}
              />
            )
        }
      })}
    </virtual-list>
    </div>
  )
}
