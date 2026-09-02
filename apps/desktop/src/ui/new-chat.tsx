import { useMemo, useState } from 'react'
import { formatAddress, type Contact } from '@messages/core'
import { C, RADIUS, TITLEBAR_HEIGHT, TYPE } from './theme'
import { Icon } from './icons'
import { Avatar, IconButton } from './primitives'
import { useShell } from './context'
import { useAppState } from './use-app-state'

function isAddress(value: string): boolean {
  return /^\+?[\d\s()-]{6,}$/.test(value) || /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value)
}

function normalise(value: string): string {
  return value.includes('@') ? value.trim().toLowerCase() : value.replace(/[^\d+]/g, '')
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

  return (
    <div testId="new-chat" style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minWidth: 0, height: '100%' }}>
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', height: TITLEBAR_HEIGHT, paddingLeft: 16, paddingRight: 8, borderBottomWidth: 1, borderColor: C.sidebarBorder, userSelect: 'none' }}>
        <text style={{ ...TYPE.title, color: C.text, flexGrow: 1 }}>New message</text>
        <IconButton icon="close" label="Cancel" testId="cancel-new-chat" onClick={onClose} />
      </div>
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', flexWrap: 'wrap', gap: 6, paddingLeft: 16, paddingRight: 16, paddingTop: 10, paddingBottom: 10, borderBottomWidth: 1, borderColor: C.sidebarBorder }}>
        <text style={{ ...TYPE.body, color: C.secondary, userSelect: 'none' }}>To:</text>
        {to.map((item) => (
          <div key={item.address} style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 4, height: 24, paddingLeft: 10, paddingRight: 6, borderRadius: 12, backgroundColor: C.selectedSoft }}>
            <text style={{ ...TYPE.caption, color: C.accent }}>{item.name ?? formatAddress(item.address)}</text>
            <IconButton icon="close" label={`Remove ${item.name ?? item.address}`} size={11} hit={18} color={C.accent} onClick={() => setTo((current) => current.filter((entry) => entry.address !== item.address))} />
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
          style={{ flexGrow: 1, minWidth: 160, ...TYPE.body, color: C.text, backgroundColor: '#00000000', borderWidth: 0 }}
        />
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minHeight: 0, overflowY: 'scroll', paddingLeft: 8, paddingRight: 8, paddingTop: 6 }}>
        {suggestions.map((item) => (
          <div
            key={item.address}
            testId={`suggest-${item.address}`}
            onClick={() => add(item.address, item.name)}
            style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10, height: 44, paddingLeft: 10, paddingRight: 10, borderRadius: RADIUS.control, cursor: 'pointer', hover: { backgroundColor: C.raised } }}
          >
            <Avatar handle={{ address: item.address, service: 'iMessage', name: item.name }} size={30} />
            <div style={{ display: 'flex', flexDirection: 'column', minWidth: 0 }}>
              <text style={{ ...TYPE.body, color: C.text }}>{item.name ?? formatAddress(item.address)}</text>
              {item.name ? <text style={{ ...TYPE.micro, color: C.secondary }}>{formatAddress(item.address)}</text> : null}
            </div>
          </div>
        ))}
        {needle && suggestions.length === 0 && isAddress(query.trim()) ? (
          <div onClick={commitQuery} style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10, height: 44, paddingLeft: 10, borderRadius: RADIUS.control, cursor: 'pointer', hover: { backgroundColor: C.raised } }}>
            <Icon name="plus" size={14} color={C.accent} />
            <text style={{ ...TYPE.body, color: C.accent }}>{`Message ${query.trim()}`}</text>
          </div>
        ) : null}
      </div>
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-end', gap: 6, paddingLeft: 12, paddingRight: 12, paddingTop: 6, paddingBottom: 10 }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-end', flexGrow: 1, minWidth: 0, borderRadius: 17, borderWidth: 1, borderColor: C.separator, backgroundColor: C.canvas, paddingLeft: 12, paddingRight: 4, paddingTop: 3, paddingBottom: 3 }}>
          <textarea
            testId="new-chat-draft"
            value={draft}
            placeholder={to.length ? 'iMessage' : 'Add someone first'}
            minRows={1}
            maxRows={6}
            theme={{ caret: C.accent, textMuted: C.tertiary }}
            style={{ flexGrow: 1, minWidth: 0, ...TYPE.bubble, color: C.text, backgroundColor: '#00000000', borderWidth: 0, paddingTop: 4, paddingBottom: 4 }}
            onChange={(event) => setDraft(event.value ?? '')}
            onSubmit={(event) => void send(event.value ?? draft)}
          />
          <div
            testId="new-chat-send"
            onClick={() => void send(draft)}
            style={{ width: 26, height: 26, borderRadius: 13, marginBottom: 1, flexShrink: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', cursor: 'pointer', backgroundColor: draft.trim() && to.length ? C.accent : C.ghost }}
          >
            <Icon name="send" size={15} color={C.onAccent} />
          </div>
        </div>
      </div>
    </div>
  )
}
