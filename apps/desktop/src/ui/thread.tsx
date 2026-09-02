import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { deliveryState, handleName, tapbackGlyph, type Attachment, type Capabilities, type Chat, type Message, type Tapback } from '@messages/core'
import { formatSeparator, formatTime, needsSeparator } from '@messages/core'
import { useAppState } from './use-app-state'
import { copyText } from '@messages/core'
import { openExternal, splitLinks } from '@messages/core'
import { BUBBLE_MAX_FRACTION, BUBBLE_MAX_WIDTH, C, RADIUS, S, THREAD_INSET, TYPE } from './theme'
import { BubbleContent } from './bubble'
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
const EMOJI_ONLY = /^(?:\p{Extended_Pictographic}️?(?:‍\p{Extended_Pictographic}️?)*\s*){1,3}$/u

/** Between two bubbles from the same person, and between two runs. */
const GAP_IN_RUN = 2
const GAP_BETWEEN_RUNS = 10
/** The avatar column in a group thread, and the gutter the hover time sits in. */
const AVATAR_COLUMN = 28
const TIME_COLUMN = 54

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
  const small = RADIUS.bubbleTight
  const top = position === 'middle' || position === 'last' ? small : big
  const bottom = position === 'middle' || position === 'first' ? small : big
  return fromMe
    ? { borderTopLeftRadius: big, borderBottomLeftRadius: big, borderTopRightRadius: top, borderBottomRightRadius: bottom }
    : { borderTopRightRadius: big, borderBottomRightRadius: big, borderTopLeftRadius: top, borderBottomLeftRadius: bottom }
}

/**
 * The bubble's fill continues past its rounded corner, then a canvas-coloured
 * quad carves the concave curve back out. Only the last bubble of a run has one.
 */
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

/**
 * Tapbacks hang off the bubble's outer top corner, the side the tail is on.
 * They overlap the corner radius, never the first line of text, so the bubble
 * carries a matching top margin whenever it has any.
 */
export const TAPBACK_LIFT = 14

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
    <div
      style={{
        position: 'absolute',
        top: -TAPBACK_LIFT,
        ...(fromMe ? { right: -2 } : { left: -2 }),
        display: 'flex',
        flexDirection: fromMe ? 'row' : 'row-reverse',
        alignItems: 'center',
      }}
    >
      {items.map((item, index) => (
        <div
          key={item.glyph}
          style={{
            display: 'flex',
            flexDirection: 'row',
            alignItems: 'center',
            gap: 2,
            height: 24,
            paddingLeft: item.count > 1 ? 7 : 5,
            paddingRight: item.count > 1 ? 7 : 5,
            borderRadius: 12,
            backgroundColor: item.mine ? C.tapbackMine : C.tapback,
            borderWidth: 2,
            borderColor: C.canvas,
            marginLeft: index === 0 ? 0 : -6,
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

function ReplyQuote({ original, fromMe }: { original: Message | undefined; fromMe: boolean }) {
  if (!original) return null
  const who = original.fromMe ? 'You' : original.sender ? handleName(original.sender) : ''
  const body = original.text || (original.attachments.length ? 'Attachment' : '')
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'stretch',
        gap: S.x2,
        marginBottom: S.x1,
        maxWidth: '100%',
        alignSelf: fromMe ? 'flex-end' : 'flex-start',
      }}
    >
      <div style={{ width: 2, borderRadius: 1, backgroundColor: fromMe ? C.imessage : C.tertiary, flexShrink: 0 }} />
      <div style={{ display: 'flex', flexDirection: 'column', flexShrink: 1, minWidth: 0 }}>
        <text style={{ ...TYPE.micro, fontWeight: 600, color: C.secondary }}>{who}</text>
        <text style={{ ...TYPE.caption, color: C.secondary, lineClamp: 2, textOverflow: 'ellipsis' }}>{body}</text>
      </div>
    </div>
  )
}

function tapbackItem(message: Message, chat: Chat, capabilities: Capabilities): MenuItem[] {
  const failed = deliveryState(message) === 'failed'
  return capabilities.reactions && !failed ? [{ kind: 'tapbacks', chatGuid: chat.guid, messageGuid: message.guid }] : []
}

function messageMenu(message: Message, chat: Chat, capabilities: Capabilities, shell: ReturnType<typeof useShell>): MenuItem[] {
  const { store } = shell
  const age = Date.now() - message.date
  const items: MenuItem[] = tapbackItem(message, chat, capabilities)
  const failed = deliveryState(message) === 'failed'
  if (failed) items.push({ label: 'Try again', icon: 'refresh', onSelect: () => void store.retry(chat.guid, message.guid) })
  if (capabilities.replies && !failed) items.push({ label: 'Reply', icon: 'reply', onSelect: () => store.setReplyingTo(chat.guid, message.guid) })
  if (message.text) items.push({ label: 'Copy', icon: 'copy', onSelect: () => void copyText(message.text) })
  for (const segment of splitLinks(message.text)) {
    if (segment.kind === 'link') items.push({ label: 'Open link', icon: 'open', onSelect: () => openExternal(segment.href) })
  }
  const image = message.attachments.find((item) => item.localPath && !item.localPath.startsWith('data:'))
  if (image?.localPath) items.push({ label: 'Open attachment', icon: 'image', onSelect: () => openExternal(image.localPath!) })
  if (message.fromMe && !failed && message.service === 'iMessage' && (capabilities.edit || capabilities.unsend)) {
    items.push({ kind: 'separator' })
    if (capabilities.edit)
      items.push({
        label: 'Edit',
        icon: 'edit',
        disabled: age > EDIT_WINDOW || message.attachments.length > 0,
        onSelect: () => store.setEditing(chat.guid, message.guid),
      })
    if (capabilities.unsend) items.push({ label: 'Undo send', icon: 'trash', danger: true, disabled: age > UNSEND_WINDOW, onSelect: () => void store.unsend(chat.guid, message.guid) })
  }
  return items
}

function attachmentMenu(attachment: Attachment, message: Message, chat: Chat, capabilities: Capabilities, shell: ReturnType<typeof useShell>): MenuItem[] {
  const { store } = shell
  const items: MenuItem[] = tapbackItem(message, chat, capabilities)
  const reveal = async () => {
    const local = attachment.localPath ?? (await store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name).catch(() => undefined))
    if (local && !local.startsWith('data:')) openExternal(local)
  }
  items.push({ label: 'Open', icon: 'open', onSelect: () => void reveal() })
  if (capabilities.replies) items.push({ label: 'Reply', icon: 'reply', onSelect: () => store.setReplyingTo(chat.guid, message.guid) })
  items.push({ label: 'Copy file name', icon: 'copy', onSelect: () => void copyText(attachment.name) })
  if (message.fromMe && capabilities.unsend) {
    items.push({ kind: 'separator' })
    items.push({
      label: 'Undo send',
      icon: 'trash',
      danger: true,
      disabled: Date.now() - message.date > UNSEND_WINDOW,
      onSelect: () => void store.unsend(chat.guid, message.guid),
    })
  }
  return items
}

/** GPUI reports `clickCount`, but a synthetic pair of clicks does not, so time them too. */
function useDoubleClick(onDouble: (event: { x?: number; y?: number }) => void) {
  const last = useRef(0)
  return (event: { x?: number; y?: number; clickCount?: number }) => {
    const now = Date.now()
    const double = (event.clickCount ?? 1) >= 2 || now - last.current < 400
    last.current = double ? 0 : now
    if (double) onDouble(event)
  }
}

const MessageRow = memo(function MessageRow({
  message,
  chat,
  position,
  showSender,
  receipt,
  capabilities,
  original,
}: {
  message: Message
  chat: Chat
  position: Position
  showSender: boolean
  receipt: string | null
  capabilities: Capabilities
  original?: Message
}) {
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
  const hasText = message.text.trim().length > 0
  const hasTapbacks = message.tapbacks.length > 0
  const lastPart = message.parts?.[message.parts.length - 1]
  const lastBlockIsText = (lastPart ? lastPart.kind === 'text' : hasText) && !emojiOnly && !message.urlPreview

  const openMessageMenu = (event: { x?: number; y?: number; isRightClick?: boolean }) => {
    if (!event.isRightClick) return
    shell.openMenu({ x: event.x ?? 0, y: event.y ?? 0, items: messageMenu(message, chat, capabilities, shell) })
  }
  const openPicker = (event: { x?: number; y?: number }) => {
    if (!capabilities.reactions || state === 'failed') return
    shell.openMenu({
      x: event.x ?? 0,
      y: (event.y ?? 0) - S.x2,
      placement: 'above',
      align: 'center',
      items: [{ kind: 'tapbacks', chatGuid: chat.guid, messageGuid: message.guid }],
    })
  }
  const onBubbleClick = useDoubleClick(openPicker)
  const time = formatTime(message.date)

  return (
    <div
      testId={`message-${message.guid}`}
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: '100%',
        paddingLeft: THREAD_INSET,
        paddingRight: THREAD_INSET,
        paddingTop: position === 'first' || position === 'single' ? GAP_BETWEEN_RUNS : GAP_IN_RUN,
      }}
    >
      {showSender && message.sender ? (
        <text
          style={{
            ...TYPE.micro,
            color: C.secondary,
            paddingLeft: (showAvatar ? AVATAR_COLUMN + S.x2 : 0) + S.x3,
            paddingBottom: 3,
            whiteSpace: 'nowrap',
            textOverflow: 'ellipsis',
          }}
        >
          {handleName(message.sender)}
        </text>
      ) : null}
      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'flex-end',
          justifyContent: fromMe ? 'flex-end' : 'flex-start',
          gap: S.x2,
          width: '100%',
        }}
      >
        {showAvatar ? (
          <div style={{ width: AVATAR_COLUMN, flexShrink: 0, display: 'flex', alignItems: 'flex-end' }}>
            {showTail ? <Avatar handle={message.sender} size={AVATAR_COLUMN} surface={C.canvas} /> : null}
          </div>
        ) : null}
        {fromMe ? (
          <text
            style={{ ...TYPE.micro, color: C.tertiary, width: TIME_COLUMN, textAlign: 'right', paddingBottom: 5, flexShrink: 0, whiteSpace: 'nowrap', opacity: hovered ? 1 : 0, userSelect: 'none' }}
          >
            {time}
          </text>
        ) : null}
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: fromMe ? 'flex-end' : 'flex-start', maxWidth: BUBBLE_MAX_FRACTION, minWidth: 0 }}>
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: fromMe ? 'flex-end' : 'flex-start', maxWidth: BUBBLE_MAX_WIDTH, minWidth: 0 }}>
            {message.replyTo ? <ReplyQuote original={original} fromMe={fromMe} /> : null}
            <div style={{ position: 'relative', marginTop: hasTapbacks && !message.replyTo ? TAPBACK_LIFT : 0, maxWidth: '100%', display: 'flex', flexDirection: 'column', alignItems: fromMe ? 'flex-end' : 'flex-start' }}>
              <BubbleContent
                message={message}
                chat={chat}
                fromMe={fromMe}
                textColor={textColor}
                handlers={{
                  radius: bubbleRadius(fromMe, position),
                  dimmed: state === 'sending',
                  paddingTop: hasTapbacks ? 11 : 7,
                  onClick: onBubbleClick,
                  onMenu: openMessageMenu,
                  onAttachmentMenu: (attachment, event) => {
                    if (event.isRightClick) shell.openMenu({ x: event.x ?? 0, y: event.y ?? 0, items: attachmentMenu(attachment, message, chat, capabilities, shell) })
                  },
                  onHover: setHovered,
                }}
              />
              {showTail && lastBlockIsText ? <Tail fromMe={fromMe} color={fill} /> : null}
              {hasTapbacks ? <Tapbacks tapbacks={message.tapbacks} fromMe={fromMe} /> : null}
            </div>
            {message.dateEdited || message.effect || receipt ? (
              <div
                style={{
                  display: 'flex',
                  flexDirection: 'row',
                  alignItems: 'center',
                  gap: S.x1,
                  paddingTop: 3,
                  paddingLeft: S.x1,
                  paddingRight: S.x1,
                  flexWrap: 'wrap',
                  justifyContent: fromMe ? 'flex-end' : 'flex-start',
                }}
              >
                {message.dateEdited ? <text style={{ ...TYPE.micro, color: C.tertiary }}>Edited</text> : null}
                {message.effect ? <text style={{ ...TYPE.micro, color: C.tertiary }}>{`Sent with ${effectName(message.effect)}`}</text> : null}
                {state === 'failed' ? <Icon name="alert" size={12} color={C.danger} /> : null}
                {receipt ? (
                  <text testId={state === 'failed' ? `failed-${message.guid}` : undefined} style={{ ...TYPE.micro, fontWeight: 600, color: state === 'failed' ? C.danger : C.tertiary }}>
                    {receipt}
                  </text>
                ) : null}
              </div>
            ) : null}
          </div>
        </div>
        {!fromMe ? (
          <text
            style={{ ...TYPE.micro, color: C.tertiary, width: TIME_COLUMN, textAlign: 'left', paddingBottom: 5, flexShrink: 0, whiteSpace: 'nowrap', opacity: hovered ? 1 : 0, userSelect: 'none' }}
          >
            {time}
          </text>
        ) : null}
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

function TypingRow({ chat }: { chat: Chat }) {
  const who = chat.participants[0]
  return (
    <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-end', gap: S.x2, width: '100%', paddingLeft: THREAD_INSET, paddingRight: THREAD_INSET, paddingTop: GAP_BETWEEN_RUNS }}>
      {chat.isGroup ? <div style={{ width: AVATAR_COLUMN, flexShrink: 0 }}>{who ? <Avatar handle={who} size={AVATAR_COLUMN} surface={C.canvas} /> : null}</div> : null}
      <div style={{ position: 'relative' }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 5, height: 34, paddingLeft: S.x3, paddingRight: S.x3, borderRadius: RADIUS.bubble, backgroundColor: C.received }}>
          {[0.35, 0.6, 1].map((opacity, index) => (
            <div key={index} style={{ width: 7, height: 7, borderRadius: 4, backgroundColor: C.secondary, opacity }} />
          ))}
        </div>
        <Tail fromMe={false} color={C.received} />
      </div>
    </div>
  )
}

function Caption({ children, top = S.x4, bottom = S.x2 }: { children: string; top?: number; bottom?: number }) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        justifyContent: 'center',
        width: '100%',
        paddingTop: top,
        paddingBottom: bottom,
        paddingLeft: S.x10,
        paddingRight: S.x10,
        userSelect: 'none',
      }}
    >
      <text style={{ ...TYPE.micro, fontWeight: 600, color: C.tertiary, textAlign: 'center' }}>{children}</text>
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
      <div
        testId="thread"
        style={{ flexGrow: 1, minHeight: 0, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: S.x3, paddingLeft: S.x6, paddingRight: S.x6 }}
      >
        <Avatar chat={chat} size={64} surface={C.canvas} />
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: S.x1 }}>
          <text style={{ ...TYPE.title, color: C.text, textAlign: 'center' }}>{chat.service === 'iMessage' ? 'iMessage' : 'Text message'}</text>
          <text style={{ ...TYPE.caption, color: C.secondary, textAlign: 'center' }}>Send the first message below.</text>
        </div>
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
        style={{ flexGrow: 1, minHeight: 0, width: '100%', paddingBottom: S.x2 }}
      >
        {rows.map((row) => {
          switch (row.kind) {
            case 'separator':
              return <Caption key={row.key}>{row.label}</Caption>
            case 'event':
              return (
                <Caption key={row.key} top={S.x3} bottom={S.x1}>
                  {row.text}
                </Caption>
              )
            case 'loading':
              return (
                <Caption key={row.key} top={S.x3} bottom={S.x1}>
                  Loading earlier messages…
                </Caption>
              )
            case 'typing':
              return <TypingRow key={row.key} chat={chat} />
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
