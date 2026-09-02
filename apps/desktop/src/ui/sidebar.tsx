import { memo, useEffect, useMemo, useState, type RefObject } from 'react'
import type { PublicInstance } from '@gpuix/react'
import { chatTitle, handleName, type Chat, type Message } from '@messages/core'
import { formatListDate } from '@messages/core'
import { useAppState } from './use-app-state'
import type { ConnectionStatus } from '@messages/core'
import { C, RADIUS, SIDEBAR_WIDTH, TITLEBAR_HEIGHT, TRAFFIC_LIGHT_CLEARANCE, TYPE } from './theme'
import { Icon } from './icons'
import { Avatar, IconButton } from './primitives'
import { useShell, type MenuItem } from './context'

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

function chatMenu(chat: Chat, shell: ReturnType<typeof useShell>): MenuItem[] {
  const { store } = shell
  return [
    { label: chat.pinned ? 'Unpin' : 'Pin', icon: chat.pinned ? 'pinOff' : 'pin', onSelect: () => store.togglePin(chat.guid) },
    { label: chat.muted ? 'Show alerts' : 'Hide alerts', icon: chat.muted ? 'unmute' : 'mute', onSelect: () => store.toggleMute(chat.guid) },
    chat.unread
      ? { label: 'Mark as read', icon: 'markRead', onSelect: () => void store.markRead(chat.guid) }
      : { label: 'Mark as unread', icon: 'markUnread', onSelect: () => void store.markUnread(chat.guid) },
    { kind: 'separator' },
    { label: 'Delete conversation', icon: 'trash', danger: true, onSelect: () => void store.deleteChat(chat.guid) },
  ]
}

const ChatRow = memo(function ChatRow({ chat, selected, onSelect }: { chat: Chat; selected: boolean; onSelect: (guid: string) => void }) {
  const shell = useShell()
  const title = chatTitle(chat)
  const preview = previewText(chat.lastMessage, chat)
  const fg = selected ? C.onAccent : C.text
  const muted = selected ? '#ffffffb3' : C.secondary
  return (
    <div
      testId={`chat-${chat.identifier}`}
      tabIndex={-1}
      onClick={() => onSelect(chat.guid)}
      onAuxClick={(event) => {
        if (event.isRightClick) shell.openMenu({ x: event.x ?? 0, y: event.y ?? 0, items: chatMenu(chat, shell) })
      }}
      onKeyDown={(event) => {
        if (event.key === 'enter') onSelect(chat.guid)
      }}
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: 8,
        height: 64,
        paddingRight: 12,
        borderRadius: RADIUS.row,
        backgroundColor: selected ? C.selected : undefined,
        cursor: 'pointer',
        hover: selected ? undefined : { backgroundColor: C.raised },
      }}
    >
      <div style={{ width: 16, display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
        {chat.unread && !selected ? <div style={{ width: 9, height: 9, borderRadius: 5, backgroundColor: C.unread }} /> : null}
      </div>
      <Avatar chat={chat} size={44} />
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minWidth: 0, gap: 1 }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 6 }}>
          <text style={{ ...TYPE.body, fontWeight: 600, color: fg, flexGrow: 1, minWidth: 0, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{title}</text>
          {chat.muted ? <Icon name="mute" size={11} color={muted} /> : null}
          <text style={{ ...TYPE.micro, color: muted, whiteSpace: 'nowrap' }}>{chat.lastActivity ? formatListDate(chat.lastActivity) : ''}</text>
        </div>
        <text style={{ ...TYPE.caption, fontSize: 12.5, lineHeight: 16, color: muted, lineClamp: 2 }}>{preview}</text>
      </div>
    </div>
  )
})

function PinnedChat({ chat, selected, onSelect }: { chat: Chat; selected: boolean; onSelect: (guid: string) => void }) {
  const shell = useShell()
  const title = chatTitle(chat)
  return (
    <div
      testId={`pinned-${chat.identifier}`}
      onClick={() => onSelect(chat.guid)}
      onAuxClick={(event) => {
        if (event.isRightClick) shell.openMenu({ x: event.x ?? 0, y: event.y ?? 0, items: chatMenu(chat, shell) })
      }}
      style={{ width: 84, display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6, paddingTop: 6, paddingBottom: 6, cursor: 'pointer' }}
    >
      <div style={{ position: 'relative', width: 64, height: 64, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <div
          style={{
            width: 64,
            height: 64,
            borderRadius: 32,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            borderWidth: 2,
            borderColor: selected ? C.accent : '#00000000',
            hover: { opacity: 0.85 },
          }}
        >
          <Avatar chat={chat} size={56} />
        </div>
        {chat.unread ? (
          <div style={{ position: 'absolute', top: 2, right: 2, width: 14, height: 14, borderRadius: 7, backgroundColor: C.unread, borderWidth: 2, borderColor: C.sidebar }} />
        ) : null}
      </div>
      <text style={{ ...TYPE.micro, fontSize: 11.5, color: selected ? C.text : C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis', maxWidth: 80 }}>
        {firstName(title)}
      </text>
    </div>
  )
}

function StatusLine({ status, host, error }: { status: ConnectionStatus; host: string; error?: string }) {
  const color = status === 'online' ? C.online : status === 'connecting' ? C.warning : C.offline
  const label = status === 'online' ? host : status === 'connecting' ? 'Connecting…' : (error ?? 'Offline')
  return (
    <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 6, minWidth: 0, flexGrow: 1 }}>
      <div style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: color, flexShrink: 0 }} />
      <text style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis', minWidth: 0 }}>{label}</text>
    </div>
  )
}

function SearchResult({ message, chat, onSelect }: { message: Message; chat: Chat; onSelect: (guid: string) => void }) {
  return (
    <div
      onClick={() => onSelect(chat.guid)}
      style={{ display: 'flex', flexDirection: 'column', gap: 2, paddingLeft: 12, paddingRight: 12, paddingTop: 8, paddingBottom: 8, borderRadius: RADIUS.row, cursor: 'pointer', hover: { backgroundColor: C.raised } }}
    >
      <div style={{ display: 'flex', flexDirection: 'row', gap: 6 }}>
        <text style={{ ...TYPE.caption, fontWeight: 600, color: C.text, flexGrow: 1, minWidth: 0, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{chatTitle(chat)}</text>
        <text style={{ ...TYPE.micro, color: C.tertiary }}>{formatListDate(message.date)}</text>
      </div>
      <text style={{ ...TYPE.caption, color: C.secondary, lineClamp: 2 }}>{message.fromMe ? `You: ${message.text}` : message.text}</text>
    </div>
  )
}

export function Sidebar({ searchRef }: { searchRef: RefObject<PublicInstance | null> }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<Message[]>([])
  const needle = query.trim().toLowerCase()

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
  const select = (guid: string) => void shell.store.selectChat(guid)
  const host = state.server ? (shell.store.transport.kind === 'demo' ? 'Demo data' : `macOS ${state.server.macosVersion ?? ''}`.trim()) : ''
  const chatByGuid = new Map(state.chats.map((chat) => [chat.guid, chat]))

  return (
    <div
      testId="sidebar"
      style={{ display: 'flex', flexDirection: 'column', width: SIDEBAR_WIDTH, flexShrink: 0, height: '100%', backgroundColor: C.sidebar, userSelect: 'none' }}
    >
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 6, height: TITLEBAR_HEIGHT, paddingLeft: 12 + TRAFFIC_LIGHT_CLEARANCE, paddingRight: 8, flexShrink: 0 }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 6, flexGrow: 1, minWidth: 0, height: 30, paddingLeft: 8, paddingRight: 8, borderRadius: RADIUS.control, backgroundColor: C.canvas, borderWidth: 1, borderColor: C.separator }}>
          <Icon name="search" size={13} color={C.tertiary} />
          <input
            ref={searchRef}
            testId="search"
            value={query}
            placeholder="Search"
            onChange={(event) => setQuery(event.value ?? '')}
            onKeyDown={(event) => {
              if (event.key === 'escape') setQuery('')
            }}
            theme={{ caret: C.accent, textMuted: C.tertiary }}
            style={{ flexGrow: 1, minWidth: 0, fontSize: 13, lineHeight: 18, color: C.text, backgroundColor: '#00000000', borderWidth: 0 }}
          />
          {query ? <IconButton icon="close" label="Clear search" size={12} hit={20} onClick={() => setQuery('')} /> : null}
        </div>
        <IconButton icon="compose" label="New message" testId="new-message" onClick={shell.startNewChat} />
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minHeight: 0, overflowY: 'scroll', paddingLeft: 8, paddingRight: 8 }}>
        {pinned.length > 0 ? (
          <div style={{ display: 'flex', flexDirection: 'row', flexWrap: 'wrap', paddingLeft: 4, paddingBottom: 6 }}>
            {pinned.map((chat) => (
              <PinnedChat key={chat.guid} chat={chat} selected={chat.guid === state.selectedChat} onSelect={select} />
            ))}
          </div>
        ) : null}
        {rest.map((chat) => (
          <ChatRow key={chat.guid} chat={chat} selected={chat.guid === state.selectedChat} onSelect={select} />
        ))}
        {needle && results.length > 0 ? (
          <div style={{ display: 'flex', flexDirection: 'column', paddingTop: 8 }}>
            <text style={{ ...TYPE.micro, fontWeight: 600, color: C.tertiary, paddingLeft: 12, paddingBottom: 4 }}>Messages</text>
            {results.map((message) => {
              const chat = chatByGuid.get(message.chatGuid)
              return chat ? <SearchResult key={message.guid} message={message} chat={chat} onSelect={select} /> : null
            })}
          </div>
        ) : null}
        {needle && visible.length === 0 && results.length === 0 ? (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 4, paddingTop: 40 }}>
            <text style={{ ...TYPE.body, fontWeight: 600, color: C.text }}>No results for “{query.trim()}”</text>
            <text style={{ ...TYPE.caption, color: C.secondary }}>Try a name, number or a word from a message.</text>
          </div>
        ) : null}
        {!needle && state.chats.length === 0 && state.status === 'online' ? (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 4, paddingTop: 40 }}>
            <text style={{ ...TYPE.body, fontWeight: 600, color: C.text }}>No conversations yet</text>
            <text style={{ ...TYPE.caption, color: C.secondary, textAlign: 'center' }}>Start one with the compose button.</text>
          </div>
        ) : null}
      </div>

      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 4, height: 40, paddingLeft: 14, paddingRight: 8, flexShrink: 0, borderTopWidth: 1, borderColor: C.sidebarBorder }}>
        <StatusLine status={state.status} host={host} error={state.error} />
        <IconButton icon="settings" label="Server settings" testId="settings" onClick={shell.openSettings} />
      </div>
    </div>
  )
}
