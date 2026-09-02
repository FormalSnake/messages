import { useMemo, useState } from 'react'
import { formatAddress, type Contact } from '@messages/core'
import { C, RADIUS, S, TITLEBAR_HEIGHT, TYPE } from './theme'
import { Icon } from './icons'
import { Avatar, IconButton } from './primitives'
import { useShell } from './context'
import { useAppState } from './use-app-state'

const FIELD_HEIGHT = 34

function isAddress(value: string): boolean {
  return /^\+?[\d\s()-]{6,}$/.test(value) || /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value)
}

function normalise(value: string): string {
  return value.includes('@') ? value.trim().toLowerCase() : value.replace(/[^\d+]/g, '')
}

function Suggestion({ address, name, onSelect }: { address: string; name?: string; onSelect: () => void }) {
  return (
    <div
      testId={`suggest-${address}`}
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key === 'enter' || event.key === 'space') onSelect()
      }}
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: S.x2,
        height: 44,
        paddingLeft: S.x2,
        paddingRight: S.x2,
        borderRadius: RADIUS.row,
        cursor: 'pointer',
        flexShrink: 0,
        hover: { backgroundColor: C.raised },
        active: { backgroundColor: C.raisedHover },
      }}
    >
      <Avatar handle={{ address, service: 'iMessage', name }} size={30} surface={C.canvas} />
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0 }}>
        <text style={{ ...TYPE.body, color: C.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{name ?? formatAddress(address)}</text>
        {name ? <text style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{formatAddress(address)}</text> : null}
      </div>
    </div>
  )
}

export function NewChat({ onClose }: { onClose: () => void }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const [query, setQuery] = useState('')
  const [to, setTo] = useState<Array<{ address: string; name?: string }>>([])
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  const needle = query.trim().toLowerCase()

  const suggestions = useMemo(() => {
    if (!needle) return []
    const chosen = new Set(to.map((item) => item.address))
    const out: Array<{ address: string; name?: string; contact?: Contact }> = []
    for (const contact of state.contacts) {
      for (const address of contact.addresses) {
        if (chosen.has(address)) continue
        if (contact.name.toLowerCase().includes(needle) || address.toLowerCase().includes(needle)) out.push({ address, name: contact.name, contact })
      }
    }
    for (const chat of state.chats) {
      if (chat.isGroup) continue
      for (const handle of chat.participants) {
        if (chosen.has(handle.address) || out.some((item) => item.address === handle.address)) continue
        if ((handle.name ?? '').toLowerCase().includes(needle) || handle.address.toLowerCase().includes(needle)) out.push({ address: handle.address, name: handle.name })
      }
    }
    return out.slice(0, 8)
  }, [needle, state.contacts, state.chats, to])

  const add = (address: string, name?: string) => {
    setTo((current) => [...current, { address, name }])
    setQuery('')
  }

  const commitQuery = () => {
    const value = query.trim()
    if (!value) return
    const first = suggestions[0]
    if (first) add(first.address, first.name)
    else if (isAddress(value)) add(normalise(value))
  }

  const send = async (text: string) => {
    const body = text.trim()
    if (!body || to.length === 0 || busy) return
    setBusy(true)
    try {
      await shell.store.createChat(to.map((item) => item.address), body)
      onClose()
    } catch {
      setBusy(false)
    }
  }

  const ready = draft.trim().length > 0 && to.length > 0 && !busy

  return (
    <div testId="new-chat" style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minWidth: 0, height: '100%' }}>
      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          height: TITLEBAR_HEIGHT,
          paddingLeft: S.x4,
          paddingRight: S.x2,
          flexShrink: 0,
          borderBottomWidth: 1,
          borderColor: C.separator,
          userSelect: 'none',
        }}
      >
        <text style={{ ...TYPE.title, color: C.text, flexGrow: 1 }}>New message</text>
        <IconButton icon="close" label="Cancel (Esc)" testId="cancel-new-chat" onClick={onClose} />
      </div>

      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          flexWrap: 'wrap',
          gap: S.x2,
          paddingLeft: S.x4,
          paddingRight: S.x4,
          paddingTop: S.x2,
          paddingBottom: S.x2,
          minHeight: 44,
          flexShrink: 0,
          borderBottomWidth: 1,
          borderColor: C.sidebarBorder,
        }}
      >
        <text style={{ ...TYPE.body, color: C.secondary, userSelect: 'none', flexShrink: 0 }}>To:</text>
        {to.map((item) => (
          <div
            key={item.address}
            style={{
              display: 'flex',
              flexDirection: 'row',
              alignItems: 'center',
              gap: 2,
              height: 24,
              paddingLeft: S.x2,
              paddingRight: S.x1,
              borderRadius: 12,
              backgroundColor: C.selectedSoft,
              flexShrink: 0,
            }}
          >
            <text style={{ ...TYPE.caption, color: C.accent, whiteSpace: 'nowrap' }}>{item.name ?? formatAddress(item.address)}</text>
            <IconButton
              icon="close"
              label={`Remove ${item.name ?? item.address}`}
              size={11}
              hit={18}
              color={C.accent}
              focusable={false}
              onClick={() => setTo((current) => current.filter((entry) => entry.address !== item.address))}
            />
          </div>
        ))}
        <input
          testId="to-field"
          value={query}
          autoFocus
          placeholder={to.length ? '' : 'Name, phone number or email'}
          onChange={(event) => setQuery(event.value ?? '')}
          onSubmit={commitQuery}
          onKeyDown={(event) => {
            if (event.key === 'backspace' && query.length === 0) setTo((current) => current.slice(0, -1))
          }}
          theme={{ caret: C.accent, textMuted: C.tertiary }}
          style={{ flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 140, height: 24, ...TYPE.body, color: C.text, backgroundColor: C.transparent, borderWidth: 0 }}
        />
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minHeight: 0, overflowY: 'scroll', paddingLeft: S.x2, paddingRight: S.x2, paddingTop: S.x2 }}>
        {suggestions.map((item) => (
          <Suggestion key={item.address} address={item.address} name={item.name} onSelect={() => add(item.address, item.name)} />
        ))}
        {needle && suggestions.length === 0 && isAddress(query.trim()) ? (
          <div
            testId="suggest-raw"
            tabIndex={0}
            onClick={commitQuery}
            onKeyDown={(event) => {
              if (event.key === 'enter' || event.key === 'space') commitQuery()
            }}
            style={{
              display: 'flex',
              flexDirection: 'row',
              alignItems: 'center',
              gap: S.x2,
              height: 44,
              paddingLeft: S.x2,
              paddingRight: S.x2,
              borderRadius: RADIUS.row,
              cursor: 'pointer',
              flexShrink: 0,
              hover: { backgroundColor: C.raised },
              active: { backgroundColor: C.raisedHover },
            }}
          >
            <div style={{ width: 30, height: 30, borderRadius: 15, backgroundColor: C.selectedSoft, display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
              <Icon name="plus" size={15} color={C.accent} strong />
            </div>
            <text style={{ ...TYPE.body, color: C.accent }}>{`Message ${query.trim()}`}</text>
          </div>
        ) : null}
        {!needle && to.length === 0 ? (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: S.x1, paddingTop: S.x10, paddingLeft: S.x4, paddingRight: S.x4 }}>
            <text style={{ ...TYPE.body, fontWeight: 600, color: C.text }}>Who is this going to?</text>
            <text style={{ ...TYPE.caption, color: C.secondary, textAlign: 'center' }}>Type a name, phone number or email above.</text>
          </div>
        ) : null}
      </div>

      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-end', gap: S.x1, paddingLeft: S.x4, paddingRight: S.x4, paddingTop: S.x2, paddingBottom: S.x3, flexShrink: 0 }}>
        <div
          style={{
            display: 'flex',
            flexDirection: 'row',
            alignItems: 'flex-end',
            gap: S.x1,
            flexGrow: 1,
            flexShrink: 1,
            flexBasis: 0,
            minWidth: 0,
            minHeight: FIELD_HEIGHT,
            borderRadius: FIELD_HEIGHT / 2,
            borderWidth: 1,
            borderColor: C.separator,
            backgroundColor: C.canvas,
            paddingLeft: S.x3,
            paddingRight: S.x1,
            paddingTop: S.x1,
            paddingBottom: S.x1,
          }}
        >
          <textarea
            testId="new-chat-draft"
            value={draft}
            placeholder={to.length ? 'iMessage' : 'Add someone first'}
            minRows={1}
            maxRows={6}
            theme={{ caret: C.accent, textMuted: C.tertiary }}
            style={{ flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, ...TYPE.bubble, color: C.text, backgroundColor: C.transparent, borderWidth: 0, paddingTop: 2, paddingBottom: 2 }}
            onChange={(event) => setDraft(event.value ?? '')}
            onSubmit={(event) => void send(event.value ?? draft)}
          />
          <div
            testId="new-chat-send"
            onClick={() => void send(draft)}
            style={{
              width: 24,
              height: 24,
              borderRadius: 12,
              flexShrink: 0,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              cursor: ready ? 'pointer' : 'default',
              backgroundColor: ready ? C.accent : C.ghost,
              opacity: ready ? 1 : 0.5,
              hover: ready ? { opacity: 0.88 } : undefined,
              active: ready ? { opacity: 0.7 } : undefined,
            }}
          >
            <Icon name="send" size={14} color={C.onAccent} strong />
          </div>
        </div>
      </div>
    </div>
  )
}
