import { memo, useCallback, useEffect, useMemo, useRef, useState, type RefObject } from 'react'
import { useGpuix, type PublicInstance } from '@gpuix/react'
import { chatTitle, handleName, type Chat, type Message } from '@messages/core'
import { formatListDate } from '@messages/core'
import { useAppState } from './use-app-state'
import type { ConnectionStatus } from '@messages/core'
import { AVATAR_ROW, C, RADIUS, ROW_HEIGHT, S, TITLEBAR_HEIGHT, TRAFFIC_LIGHT_CLEARANCE, TYPE } from './theme'
import { Icon } from './icons'
import { Avatar, IconButton, ring } from './primitives'
import { shortcut, useShell, type MenuItem } from './context'

/** The dot column and the row's right inset, so every row lines up on two edges. */
const DOT_COLUMN = 16
const ROW_INSET = 10
const PINNED_CELL = 92

interface RowProps {
  chat: Chat
  selected: boolean
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
    chat.unread
      ? { label: 'Mark as read', icon: 'markRead', onSelect: () => void store.markRead(chat.guid) }
      : { label: 'Mark as unread', icon: 'markUnread', shortcut: shortcut('U', { shift: true }), onSelect: () => void store.markUnread(chat.guid) },
    { label: 'Show details', icon: 'info', shortcut: shortcut('I'), onSelect: () => void store.selectChat(chat.guid).then(() => shell.setInfo(true)) },
    { kind: 'separator' },
    { label: 'Delete conversation', icon: 'trash', danger: true, onSelect: () => void store.deleteChat(chat.guid) },
  )
  return items
}

const ChatRow = memo(function ChatRow({ chat, selected, cursored, onSelect, onArrow, register }: RowProps) {
  const shell = useShell()
  const title = chatTitle(chat)
  const preview = previewText(chat.lastMessage, chat)
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
        {chat.unread && !selected ? <div style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: C.unread }} /> : null}
      </div>
      <Avatar chat={chat} size={AVATAR_ROW} surface={selected ? C.selected : C.sidebar} />
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, gap: 1 }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x1 }}>
          <text style={{ ...TYPE.body, fontWeight: 600, color: fg, flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>
            {title}
          </text>
          {chat.muted ? <Icon name="mute" size={11} color={muted} /> : null}
          <text style={{ ...TYPE.micro, color: muted, whiteSpace: 'nowrap', flexShrink: 0 }}>{chat.lastActivity ? formatListDate(chat.lastActivity) : ''}</text>
        </div>
        <text style={{ ...TYPE.preview, color: muted, lineClamp: 2, textOverflow: 'ellipsis', width: '100%', minWidth: 0 }}>{preview}</text>
      </div>
    </div>
  )
})

function PinnedChat({ chat, selected, cursored, onSelect, onArrow, register }: RowProps) {
  const shell = useShell()
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
          <Avatar chat={chat} size={52} surface={C.sidebar} />
        </div>
        {chat.unread ? (
          <div style={{ position: 'absolute', top: 0, left: 0, width: 14, height: 14, borderRadius: 7, backgroundColor: C.unread, borderWidth: 2, borderColor: C.sidebar }} />
        ) : null}
      </div>
      <text style={{ ...TYPE.micro, color: selected ? C.text : C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis', maxWidth: PINNED_CELL - S.x2, textAlign: 'center' }}>
        {firstName(title)}
      </text>
    </div>
  )
}

function StatusLine({ status, host, error }: { status: ConnectionStatus; host: string; error?: string }) {
  const color = status === 'online' ? C.online : status === 'connecting' ? C.warning : C.offline
  const label = status === 'online' ? host : status === 'connecting' ? 'Connecting…' : (error ?? 'Offline')
  return (
    <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x2, minWidth: 0, flexGrow: 1 }}>
      <div style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: color, flexShrink: 0 }} />
      <text style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis', minWidth: 0 }}>{label}</text>
    </div>
  )
}

function SearchResult({ message, chat, onSelect }: { message: Message; chat: Chat; onSelect: (guid: string) => void }) {
  return (
    <div
      tabIndex={0}
      onClick={() => onSelect(chat.guid)}
      onKeyDown={(event) => {
        if (event.key === 'enter' || event.key === 'space') onSelect(chat.guid)
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
      <text style={{ ...TYPE.preview, color: C.secondary, lineClamp: 2, textOverflow: 'ellipsis', width: '100%', minWidth: 0 }}>{message.fromMe ? `You: ${message.text}` : message.text}</text>
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
  const needle = query.trim().toLowerCase()

  const register = useCallback((guid: string, instance: PublicInstance | null) => {
    if (instance) rows.current.set(guid, instance)
    else rows.current.delete(guid)
  }, [])

  const onArrow = useCallback(
    (guid: string, delta: number) => {
      const index = order.current.indexOf(guid)
      const next = order.current[Math.min(Math.max(index + delta, 0), order.current.length - 1)]
      if (!next || next === guid) return
      setCursor(next)
      const instance = rows.current.get(next)
      if (instance && renderer?.focusElement) renderer.focusElement(instance.id)
    },
    [renderer],
  )

  useEffect(() => {
    if (needle.length < 2) {
      setResults([])
      return
    }
    let cancelled = false
    const timer = setTimeout(() => {
      shell.store.transport
        .searchMessages(needle, { limit: 20 })
        .then((found) => {
          if (!cancelled) setResults(found)
        })
        .catch(() => undefined)
    }, 250)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [needle, shell.store])

  const visible = useMemo(() => {
    if (!needle) return state.chats
    return state.chats.filter((chat) => {
      const haystack = [chatTitle(chat), chat.identifier, ...chat.participants.map((p) => `${p.address} ${p.name ?? ''}`)].join(' ').toLowerCase()
      return haystack.includes(needle)
    })
  }, [state.chats, needle])

  const pinned = needle ? [] : visible.filter((chat) => chat.pinned)
  const rest = needle ? visible : visible.filter((chat) => !chat.pinned)
  order.current = [...pinned, ...rest].map((chat) => chat.guid)
  const select = (guid: string) => {
    setCursor(guid)
    const instance = rows.current.get(guid)
    if (instance && renderer?.focusElement) renderer.focusElement(instance.id)
    void shell.store.selectChat(guid)
  }
  const host = state.server ? (shell.store.transport.kind === 'demo' ? 'Demo data' : `macOS ${state.server.macosVersion ?? ''}`.trim()) : ''
  const chatByGuid = new Map(state.chats.map((chat) => [chat.guid, chat]))
  const backdropMenu = (event: { x?: number; y?: number; isRightClick?: boolean }) => {
    if (!event.isRightClick) return
    shell.openMenu({
      x: event.x ?? 0,
      y: event.y ?? 0,
      items: [{ label: 'New message', icon: 'compose', shortcut: shortcut('N'), onSelect: shell.startNewChat }],
    })
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
            onKeyDown={(event) => {
              if (event.key === 'escape') setQuery('')
              else if (event.key === 'down') {
                const first = order.current[0]
                if (first) {
                  setCursor(first)
                  const instance = rows.current.get(first)
                  if (instance && renderer?.focusElement) renderer.focusElement(instance.id)
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

      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minHeight: 0, overflowY: 'scroll', paddingLeft: S.x2, paddingRight: S.x2, paddingBottom: S.x2 }}>
        {pinned.length > 0 ? (
          <>
            <div style={{ display: 'flex', flexDirection: 'row', flexWrap: 'wrap', paddingTop: S.x1, paddingBottom: S.x2 }}>
              {pinned.map((chat) => (
                <PinnedChat
                  key={chat.guid}
                  chat={chat}
                  selected={chat.guid === state.selectedChat}
                  cursored={chat.guid === cursor}
                  onSelect={select}
                  onArrow={onArrow}
                  register={register}
                />
              ))}
            </div>
            {rest.length > 0 ? <div style={{ height: 1, backgroundColor: C.sidebarBorder, marginBottom: S.x2, marginLeft: ROW_INSET, marginRight: ROW_INSET, flexShrink: 0 }} /> : null}
          </>
        ) : null}
        {rest.map((chat) => (
          <ChatRow
            key={chat.guid}
            chat={chat}
            selected={chat.guid === state.selectedChat}
            cursored={chat.guid === cursor}
            onSelect={select}
            onArrow={onArrow}
            register={register}
          />
        ))}
        {needle && results.length > 0 ? (
          <div style={{ display: 'flex', flexDirection: 'column', paddingTop: S.x3, flexShrink: 0 }}>
            <text style={{ ...TYPE.micro, fontWeight: 600, color: C.tertiary, paddingLeft: ROW_INSET, paddingBottom: S.x1 }}>Messages</text>
            {results.map((message) => {
              const chat = chatByGuid.get(message.chatGuid)
              return chat ? <SearchResult key={message.guid} message={message} chat={chat} onSelect={select} /> : null
            })}
          </div>
        ) : null}
        {needle && visible.length === 0 && results.length === 0 ? (
          <EmptyNote title={`No results for “${query.trim()}”`} body="Try a name, number or a word from a message." />
        ) : null}
        {!needle && state.chats.length === 0 && state.status === 'online' ? (
          <EmptyNote title="No conversations yet" body="Start one with the compose button." />
        ) : null}
        <div testId="sidebar-backdrop" onAuxClick={backdropMenu} style={{ flexGrow: 1, minHeight: S.x10 }} />
      </div>

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
        <StatusLine status={state.status} host={host} error={state.error} />
        <IconButton icon="settings" label={`Server settings (${shortcut(',')})`} testId="settings" onClick={shell.openSettings} />
      </div>
    </div>
  )
}
