import { memo, useCallback, useEffect, useMemo, useRef, useState, type RefObject } from 'react'
import { motion, useGpuix, useWindowSize, type PublicInstance } from '@gpuix/react'
import { chatTitle, conversationChats, conversationFocus, conversationGuid, conversationTyping, conversationUnread, handleName, parseSearchQuery, resolveSearchQuery, type Chat, type Message } from '@messages/core'
import { formatListDate } from '@messages/core'
import { useAppState } from './use-app-state'
import type { ConnectionStatus } from '@messages/core'
import { AVATAR_ROW, C, RADIUS, ROW_HEIGHT, S, TITLEBAR_HEIGHT, TRAFFIC_LIGHT_CLEARANCE, TYPE } from './theme'
import { Icon } from './icons'
import { Avatar, IconButton, ring } from './primitives'
import { shortcut, useShell, type MenuItem } from './context'
import { DURATION, EASE_OUT, usePresence } from './motion'

/** The dot column and the row's right inset, so every row lines up on two edges. */
const DOT_COLUMN = 16
const ROW_INSET = 10
const PINNED_CELL = 92
/** The unread badge on a pin, centred on the picture's edge the way Messages puts it. */
const BADGE = 16
const FOOTER_HEIGHT = 38
/**
 * The list is windowed on the React side: only rows near the viewport exist
 * as elements, since gpuix rebuilds every element it holds each frame and a
 * few hundred conversations cost more per frame than the whole thread.
 */
const WINDOW_MARGIN = 10
const INITIAL_WINDOW = 30

/** One row of the sidebar list. Pins sit above the list, so every row here is about one row tall. */
type SidebarItem =
  | { kind: 'chat'; key: string; chat: Chat }
  | { kind: 'results'; key: string }
  | { kind: 'result'; key: string; message: Message; chat: Chat }
  | { kind: 'note'; key: string; title: string; body: string }
  | { kind: 'backdrop'; key: string }

interface RowProps {
  chat: Chat
  selected: boolean
  typing?: boolean
  unread?: boolean
  /** The other person has a Focus on. The list reads it; the row only paints the moon. */
  silenced?: boolean
  /** The row the keyboard is on. gpuix has no focus event, so the list owns this. */
  cursored: boolean
  onSelect: (guid: string) => void
  onArrow: (guid: string, delta: number) => void
  register: (guid: string, instance: PublicInstance | null) => void
}

function firstName(name: string): string {
  return name.split(/\s+/)[0] ?? name
}

export function previewText(message: Message | undefined, chat: Chat): string {
  if (!message) return chat.isGroup ? 'New group' : 'New conversation'
  const who = message.fromMe ? 'You' : message.sender ? firstName(handleName(message.sender)) : ''
  if (message.dateRetracted) return `${who} unsent a message`
  if (message.groupEvent) {
    switch (message.groupEvent.kind) {
      case 'rename':
        return `${who} named the conversation “${message.groupEvent.title}”`
      case 'join':
        return `${who} added ${message.groupEvent.who ? handleName(message.groupEvent.who) : 'someone'}`
      case 'leave':
        return `${message.groupEvent.who ? handleName(message.groupEvent.who) : who} left the conversation`
      case 'photo':
        return `${who} changed the group photo`
    }
  }
  let body = message.text
  if (!body && message.attachments.length > 0) {
    const first = message.attachments[0]
    body = message.isAudio ? 'Audio message' : first?.mime.startsWith('image/') ? 'Photo' : first?.mime.startsWith('video/') ? 'Video' : (first?.name ?? 'Attachment')
  }
  if (chat.isGroup && !message.fromMe && who) return `${who}: ${body}`
  return body
}

export function chatMenu(chat: Chat, shell: ReturnType<typeof useShell>, options: { open?: boolean } = {}): MenuItem[] {
  const { store } = shell
  const items: MenuItem[] = []
  if (options.open) items.push({ label: 'Open conversation', icon: 'conversation', onSelect: () => void store.selectChat(chat.guid) })
  items.push(
    { label: chat.pinned ? 'Unpin' : 'Pin', icon: chat.pinned ? 'pinOff' : 'pin', onSelect: () => store.togglePin(chat.guid) },
    { label: chat.muted ? 'Show alerts' : 'Hide alerts', icon: chat.muted ? 'unmute' : 'mute', onSelect: () => store.toggleMute(chat.guid) },
    {
      label: chat.readReceipts === false ? 'Send read receipts' : 'Read without receipts',
      icon: chat.readReceipts === false ? 'eye' : 'eyeOff',
      onSelect: () => store.toggleReadReceipts(chat.guid),
    },
    chat.unread
      ? { label: 'Mark as read', icon: 'markRead', onSelect: () => void store.markRead(chat.guid) }
      : { label: 'Mark as unread', icon: 'markUnread', shortcut: shortcut('U', { shift: true }), onSelect: () => void store.markUnread(chat.guid) },
    { label: 'Show details', icon: 'info', shortcut: shortcut('I'), onSelect: () => void store.selectChat(chat.guid).then(() => shell.setInfo(true)) },
    { label: 'Export conversation…', icon: 'download', onSelect: () => void store.exportConversation(chat.guid) },
    { kind: 'separator' },
    { label: 'Delete conversation', icon: 'trash', danger: true, onSelect: () => confirmDelete(chat, shell) },
  )
  return items
}

export function confirmDelete(chat: Chat, shell: ReturnType<typeof useShell>): void {
  shell.confirm({
    title: `Delete “${chatTitle(chat)}”?`,
    body: 'The conversation is removed on your Mac and on every device that syncs with it.',
    action: 'Delete',
    danger: true,
    onConfirm: () => void shell.store.deleteChat(chat.guid),
  })
}

const ChatRow = memo(function ChatRow({ chat, selected, typing = false, unread = chat.unread, silenced = false, cursored, onSelect, onArrow, register }: RowProps) {
  const shell = useShell()
  const dot = unread && !selected
  const showDot = usePresence(dot, DURATION.fast)
  const title = chatTitle(chat)
  const preview = typing ? 'Typing…' : previewText(chat.lastMessage, chat)
  const fg = selected ? C.onAccent : C.text
  const muted = selected ? C.onAccentSoft : C.secondary
  return (
    <div
      testId={`chat-${chat.identifier}`}
      ref={(instance) => register(chat.guid, instance)}
      tabIndex={0}
      onClick={() => onSelect(chat.guid)}
      onAuxClick={(event) => {
        if (event.isRightClick) shell.openMenu({ x: event.x ?? 0, y: event.y ?? 0, items: chatMenu(chat, shell, { open: !selected }) })
      }}
      onKeyDown={(event) => {
        if (event.key === 'enter' || event.key === 'space') onSelect(chat.guid)
        else if (event.key === 'down') onArrow(chat.guid, 1)
        else if (event.key === 'up') onArrow(chat.guid, -1)
      }}
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: S.x2,
        height: ROW_HEIGHT,
        paddingRight: ROW_INSET,
        borderRadius: RADIUS.row,
        backgroundColor: selected ? C.selected : undefined,
        cursor: 'pointer',
        flexShrink: 0,
        ...ring(cursored, selected ? C.text : C.focusRing),
        hover: selected ? undefined : { backgroundColor: C.raised },
        active: selected ? undefined : { backgroundColor: C.raisedHover },
      }}
    >
      <div style={{ width: DOT_COLUMN, display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
        {showDot ? (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: dot ? 1 : 0 }}
            transition={{ duration: DURATION.fast, ease: EASE_OUT }}
            style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: C.unread, pointerEvents: 'none' }}
          />
        ) : null}
      </div>
      <Avatar chat={chat} size={AVATAR_ROW} />
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, gap: 1 }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x1 }}>
          <text style={{ ...TYPE.body, fontWeight: 600, color: fg, flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>
            {title}
          </text>
          {silenced ? <Icon name="silenced" size={11} color={muted} /> : null}
          {chat.muted ? <Icon name="mute" size={11} color={muted} /> : null}
          <text style={{ ...TYPE.micro, color: muted, whiteSpace: 'nowrap', flexShrink: 0 }}>{chat.lastActivity ? formatListDate(chat.lastActivity) : ''}</text>
        </div>
        <text style={{ ...TYPE.preview, color: typing && !selected ? C.accent : muted, lineClamp: 2, textOverflow: 'ellipsis', width: '100%', minWidth: 0 }}>{preview}</text>
      </div>
    </div>
  )
})

function PinnedChat({ chat, selected, unread = chat.unread, cursored, onSelect, onArrow, register }: RowProps) {
  const shell = useShell()
  const showBadge = usePresence(unread, DURATION.fast)
  const title = chatTitle(chat)
  return (
    <div
      testId={`pinned-${chat.identifier}`}
      ref={(instance) => register(chat.guid, instance)}
      tabIndex={0}
      onClick={() => onSelect(chat.guid)}
      onKeyDown={(event) => {
        if (event.key === 'enter' || event.key === 'space') onSelect(chat.guid)
        else if (event.key === 'right' || event.key === 'down') onArrow(chat.guid, 1)
        else if (event.key === 'left' || event.key === 'up') onArrow(chat.guid, -1)
      }}
      onAuxClick={(event) => {
        if (event.isRightClick) shell.openMenu({ x: event.x ?? 0, y: event.y ?? 0, items: chatMenu(chat, shell, { open: !selected }) })
      }}
      style={{
        width: PINNED_CELL,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: S.x1,
        paddingTop: S.x2,
        paddingBottom: S.x2,
        borderRadius: RADIUS.row,
        cursor: 'pointer',
        ...ring(cursored),
        hover: { backgroundColor: C.raised },
        active: { backgroundColor: C.raisedHover },
      }}
    >
      <div style={{ position: 'relative', width: 60, height: 60, display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
        <div
          style={{
            width: 60,
            height: 60,
            borderRadius: 30,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            borderWidth: 2,
            borderColor: selected ? C.accent : C.transparent,
          }}
        >
          <Avatar chat={chat} size={52} />
        </div>
        {showBadge ? (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: unread ? 1 : 0 }}
            transition={{ duration: DURATION.fast, ease: EASE_OUT }}
            style={{ position: 'absolute', top: 3, right: 3, width: BADGE, height: BADGE, borderRadius: BADGE / 2, backgroundColor: C.unread, borderWidth: 2, borderColor: C.sidebar, pointerEvents: 'none' }}
          />
        ) : null}
      </div>
      <text style={{ ...TYPE.micro, color: selected ? C.text : C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis', maxWidth: PINNED_CELL - S.x2, textAlign: 'center' }}>
        {chat.isGroup ? title : firstName(title)}
      </text>
    </div>
  )
}

function StatusLine({ status, host, pending }: { status: ConnectionStatus; host: string; pending: number }) {
  const color = status === 'online' ? C.online : status === 'connecting' ? C.warning : C.offline
  const queued = pending > 0 ? ` · ${pending === 1 ? '1 message waiting' : `${pending} messages waiting`}` : ''
  const label = status === 'online' ? host + queued : status === 'connecting' ? `Connecting…${queued}` : `Offline, retrying…${queued}`
  return (
    <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x2, minWidth: 0, flexGrow: 1 }}>
      <div style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: color, flexShrink: 0 }} />
      <text style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis', minWidth: 0 }}>{label}</text>
    </div>
  )
}

function SearchResult({ message, chat, onOpen }: { message: Message; chat: Chat; onOpen: () => void }) {
  return (
    <div
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(event) => {
        if (event.key === 'enter' || event.key === 'space') onOpen()
      }}
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 1,
        paddingLeft: ROW_INSET,
        paddingRight: ROW_INSET,
        paddingTop: S.x2,
        paddingBottom: S.x2,
        borderRadius: RADIUS.row,
        cursor: 'pointer',
        flexShrink: 0,
        hover: { backgroundColor: C.raised },
        active: { backgroundColor: C.raisedHover },
      }}
    >
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x1 }}>
        <text style={{ ...TYPE.body, fontWeight: 600, color: C.text, flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>
          {chatTitle(chat)}
        </text>
        <text style={{ ...TYPE.micro, color: C.tertiary, whiteSpace: 'nowrap', flexShrink: 0 }}>{formatListDate(message.date)}</text>
      </div>
      <text style={{ ...TYPE.preview, color: C.secondary, lineClamp: 2, textOverflow: 'ellipsis', width: '100%', minWidth: 0 }}>{previewText(message, chat)}</text>
    </div>
  )
}

const OPERATOR_TIPS = ['from:name or from:me', 'has:photo, has:video, has:file or has:link', 'before:2024-01-01, after:2024-01-01', 'in:chat name'] as const

function SearchTips() {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 2, paddingLeft: ROW_INSET, paddingRight: ROW_INSET, paddingTop: S.x1, paddingBottom: S.x2, flexShrink: 0 }}>
      <text style={{ ...TYPE.micro, fontWeight: 600, color: C.tertiary }}>Search tips</text>
      {OPERATOR_TIPS.map((tip) => (
        <text key={tip} style={{ ...TYPE.caption, color: C.secondary }}>
          {tip}
        </text>
      ))}
    </div>
  )
}

function EmptyNote({ title, body }: { title: string; body: string }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: S.x1, paddingTop: S.x10, paddingLeft: S.x4, paddingRight: S.x4 }}>
      <text style={{ ...TYPE.body, fontWeight: 600, color: C.text, textAlign: 'center' }}>{title}</text>
      <text style={{ ...TYPE.caption, color: C.secondary, textAlign: 'center' }}>{body}</text>
    </div>
  )
}

export function Sidebar({ searchRef, width }: { searchRef: RefObject<PublicInstance | null>; width: number }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const { renderer } = useGpuix()
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<Message[]>([])
  const [cursor, setCursor] = useState<string | null>(null)
  const rows = useRef(new Map<string, PublicInstance | null>())
  const order = useRef<string[]>([])
  const listRef = useRef<PublicInstance | null>(null)
  const items = useRef<SidebarItem[]>([])
  const [range, setRange] = useState({ start: 0, end: INITIAL_WINDOW })
  const rangeRef = useRef(range)
  /** A row the keyboard asked for while it was outside the window; it takes focus when it mounts. */
  const pendingFocus = useRef<string | null>(null)
  const { height: windowHeight } = useWindowSize()
  const listHeight = Math.max(ROW_HEIGHT * 2, windowHeight - TITLEBAR_HEIGHT - FOOTER_HEIGHT)
  const trimmed = query.trim()
  const hasQuery = trimmed.length > 0
  const parsed = useMemo(() => parseSearchQuery(trimmed), [trimmed])
  const freeText = parsed.text.trim().toLowerCase()
  const hasFilter = parsed.fromMe || parsed.senders.length > 0 || Boolean(parsed.attachments) || parsed.links || parsed.before !== undefined || parsed.after !== undefined || parsed.chatNames.length > 0

  /**
   * gpui scrolls a `List` to reveal whatever takes focus, and it lands the row
   * against the bottom edge even when the row was already on screen, so a click
   * on a row halfway down jumped the list. The list is scrolled here or not at
   * all: read the anchor, focus, put it back.
   */
  const focusRow = useCallback(
    (instance: PublicInstance) => {
      const listId = listRef.current?.id
      const anchor = listId != null ? (renderer?.getListScrollTop?.(listId) ?? null) : null
      renderer?.focusElement?.(instance.id)
      const index = anchor?.[0]
      if (listId == null || index === undefined || index >= items.current.length) return
      renderer?.scrollToItem?.(listId, index, anchor?.[1] ?? 0)
    },
    [renderer],
  )

  const register = useCallback(
    (guid: string, instance: PublicInstance | null) => {
      if (instance) rows.current.set(guid, instance)
      else rows.current.delete(guid)
      if (instance && pendingFocus.current === guid && renderer?.focusElement) {
        pendingFocus.current = null
        focusRow(instance)
      }
    },
    [renderer, focusRow],
  )

  /** Focuses a row, scrolling it into the window first when the keyboard has walked off screen. */
  const reveal = useCallback(
    (guid: string, delta: number) => {
      const instance = rows.current.get(guid)
      if (instance) focusRow(instance)
      else pendingFocus.current = guid
      const index = items.current.findIndex((item) => item.kind === 'chat' && item.chat.guid === guid)
      const listId = listRef.current?.id
      if (index < 0 || listId == null || !renderer?.scrollToItem) return
      const { start, end } = rangeRef.current
      if (instance && index >= start && index < end - 1) return
      renderer.scrollToItem(listId, index, delta > 0 ? -(listHeight - ROW_HEIGHT - S.x2) : 0)
    },
    [renderer, focusRow, listHeight],
  )

  const onArrow = useCallback(
    (guid: string, delta: number) => {
      const index = order.current.indexOf(guid)
      const next = order.current[Math.min(Math.max(index + delta, 0), order.current.length - 1)]
      if (!next || next === guid) return
      setCursor(next)
      reveal(next, delta)
    },
    [reveal],
  )

  useEffect(() => {
    if (freeText.length < 2 && !hasFilter) {
      setResults([])
      return
    }
    let cancelled = false
    const timer = setTimeout(() => {
      const { text, filters } = resolveSearchQuery(parsed, { contacts: state.contacts, chats: state.chats })
      shell.store.transport
        .searchMessages(text, { ...filters, limit: 20 })
        .then((found) => {
          if (!cancelled) setResults(found)
        })
        .catch(() => undefined)
    }, 250)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [parsed, freeText, hasFilter, state.contacts, state.chats, shell.store])

  const people = useMemo(() => conversationChats(state), [state.chats, state.primaryOf])
  const visible = useMemo(() => {
    if (!freeText) return people
    return people.filter((chat) => {
      const haystack = [chatTitle(chat), chat.identifier, ...chat.participants.map((p) => `${p.address} ${p.name ?? ''}`)].join(' ').toLowerCase()
      return haystack.includes(freeText)
    })
  }, [people, freeText])

  const pinned = hasQuery ? [] : visible.filter((chat) => chat.pinned)
  const rest = hasQuery ? visible : visible.filter((chat) => !chat.pinned)
  order.current = [...pinned, ...rest].map((chat) => chat.guid)
  // Stable, so the memoised rows survive the sidebar re-rendering on every store change.
  const select = useCallback(
    (guid: string) => {
      setCursor(guid)
      const instance = rows.current.get(guid)
      if (instance) focusRow(instance)
      void shell.store.selectChat(guid)
    },
    [focusRow, shell.store],
  )
  // Only for a plain-text query with nothing found: someone already using an operator knows the syntax.
  const showTips = hasQuery && !hasFilter && visible.length === 0 && results.length === 0 && /^[a-zA-Z]/.test(trimmed)
  const host = state.server ? (shell.store.transport.kind === 'demo' ? 'Demo data' : `macOS ${state.server.macosVersion ?? ''}`.trim()) : ''
  const backdropMenu = (event: { x?: number; y?: number; isRightClick?: boolean }) => {
    if (!event.isRightClick) return
    shell.openMenu({
      x: event.x ?? 0,
      y: event.y ?? 0,
      items: [{ label: 'New message', icon: 'compose', shortcut: shortcut('N'), onSelect: shell.startNewChat }],
    })
  }

  const list: SidebarItem[] = []
  for (const chat of rest) list.push({ kind: 'chat', key: chat.guid, chat })
  if (hasQuery && results.length > 0) {
    // Only search results need this, and the sidebar re-renders on every store
    // change, so it is not worth a few hundred entries the rest of the time.
    const chatByGuid = new Map(state.chats.map((chat) => [chat.guid, chat]))
    list.push({ kind: 'results', key: 'results' })
    for (const message of results) {
      const chat = chatByGuid.get(conversationGuid(state, message.chatGuid))
      if (chat) list.push({ kind: 'result', key: `result-${message.guid}`, message, chat })
    }
  }
  if (hasQuery && visible.length === 0 && results.length === 0) list.push({ kind: 'note', key: 'no-results', title: `No results for “${trimmed}”`, body: 'Try a name, number or a word from a message.' })
  if (!hasQuery && state.chats.length === 0 && state.status === 'online') list.push({ kind: 'note', key: 'empty', title: 'No conversations yet', body: 'Start one with the compose button.' })
  list.push({ kind: 'backdrop', key: 'backdrop' })
  items.current = list
  const count = list.length
  // The reported range can be stale after a search shrinks the list; clamping keeps the window inside it.
  const windowStart = Math.max(0, Math.min(range.start, count) - WINDOW_MARGIN)
  const windowEnd = Math.min(count, Math.max(range.end, range.start + 1) + WINDOW_MARGIN)

  const renderItem = (item: SidebarItem) => {
    switch (item.kind) {
      case 'chat':
        return (
          <ChatRow
            chat={item.chat}
            selected={item.chat.guid === state.selectedChat}
            typing={conversationTyping(state, item.chat.guid)}
            unread={conversationUnread(state, item.chat.guid)}
            silenced={conversationFocus(state, item.chat.guid) === 'silenced'}
            cursored={item.chat.guid === cursor}
            onSelect={select}
            onArrow={onArrow}
            register={register}
          />
        )
      case 'results':
        return <text style={{ ...TYPE.micro, fontWeight: 600, color: C.tertiary, paddingLeft: ROW_INSET, paddingTop: S.x3, paddingBottom: S.x1 }}>Messages</text>
      case 'result':
        return <SearchResult message={item.message} chat={item.chat} onOpen={() => shell.jumpTo(item.message.chatGuid, item.message.guid)} />
      case 'note':
        return <EmptyNote title={item.title} body={item.body} />
      case 'backdrop':
        return <div testId="sidebar-backdrop" onAuxClick={backdropMenu} style={{ height: S.x10 }} />
    }
  }

  return (
    <div
      testId="sidebar"
      style={{ display: 'flex', flexDirection: 'column', width, flexShrink: 0, height: '100%', backgroundColor: C.sidebar, userSelect: 'none' }}
    >
      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          gap: S.x2,
          height: TITLEBAR_HEIGHT,
          paddingLeft: S.x3 + TRAFFIC_LIGHT_CLEARANCE,
          paddingRight: S.x2,
          flexShrink: 0,
        }}
      >
        <div
          style={{
            display: 'flex',
            flexDirection: 'row',
            alignItems: 'center',
            gap: S.x1,
            flexGrow: 1,
            flexShrink: 1,
            flexBasis: 0,
            minWidth: 0,
            height: 28,
            paddingLeft: S.x2,
            paddingRight: query ? S.x1 : S.x2,
            borderRadius: RADIUS.control,
            backgroundColor: C.canvas,
            borderWidth: 1,
            borderColor: C.separator,
          }}
        >
          <Icon name="search" size={13} color={C.tertiary} />
          <input
            ref={searchRef}
            testId="search"
            value={query}
            placeholder="Search"
            onChange={(event) => setQuery(event.value ?? '')}
            onSubmit={() => {
              const target = cursor ?? order.current[0]
              if (target) void shell.store.selectChat(target)
            }}
            onKeyDown={(event) => {
              if (event.key === 'escape') setQuery('')
              else if (event.key === 'down') {
                const first = order.current[0]
                if (first) {
                  setCursor(first)
                  reveal(first, -1)
                }
              }
            }}
            theme={{ caret: C.accent, textMuted: C.tertiary }}
            style={{ flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, ...TYPE.body, color: C.text, backgroundColor: C.transparent, borderWidth: 0 }}
          />
          {query ? <IconButton icon="close" label="Clear search" size={12} hit={20} focusable={false} onClick={() => setQuery('')} /> : null}
        </div>
        <IconButton icon="compose" label="New message" testId="new-message" size={17} onClick={shell.startNewChat} />
      </div>

      {showTips ? <SearchTips /> : null}

      {pinned.length > 0 ? (
        <div style={{ display: 'flex', flexDirection: 'column', paddingLeft: S.x2, paddingRight: S.x2, flexShrink: 0 }}>
          <div style={{ display: 'flex', flexDirection: 'row', flexWrap: 'wrap', paddingTop: S.x1, paddingBottom: S.x2 }}>
            {pinned.map((chat) => (
              <PinnedChat
                key={chat.guid}
                chat={chat}
                selected={chat.guid === state.selectedChat}
                unread={conversationUnread(state, chat.guid)}
                cursored={chat.guid === cursor}
                onSelect={select}
                onArrow={onArrow}
                register={register}
              />
            ))}
          </div>
          {rest.length > 0 ? <div style={{ height: 1, backgroundColor: C.sidebarBorder, marginBottom: S.x2, marginLeft: ROW_INSET, marginRight: ROW_INSET, flexShrink: 0 }} /> : null}
        </div>
      ) : null}

      <virtual-list
        ref={listRef}
        itemCount={count}
        windowStart={windowStart}
        estimatedItemHeight={ROW_HEIGHT}
        overdraw={400}
        onVisibleRange={(event) => {
          const start = event.startIndex ?? 0
          const end = event.endIndex ?? start
          rangeRef.current = { start, end }
          setRange((current) => (current.start === start && current.end === end ? current : { start, end }))
        }}
        style={{ flexGrow: 1, minHeight: 0, width: '100%', paddingBottom: S.x2 }}
      >
        {items.current.slice(windowStart, windowEnd).map((item) => (
          <div key={item.key} style={{ display: 'flex', flexDirection: 'column', paddingLeft: S.x2, paddingRight: S.x2 }}>
            {renderItem(item)}
          </div>
        ))}
      </virtual-list>

      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          gap: S.x1,
          height: 38,
          paddingLeft: S.x3,
          paddingRight: S.x2,
          flexShrink: 0,
          borderTopWidth: 1,
          borderColor: C.sidebarBorder,
        }}
      >
        <StatusLine status={state.status} host={host} pending={shell.store.pendingSends} />
        <IconButton icon="settings" label={`Server settings (${shortcut(',')})`} testId="settings" onClick={shell.openSettings} />
      </div>
    </div>
  )
}
