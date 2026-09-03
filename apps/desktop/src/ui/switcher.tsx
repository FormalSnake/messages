import { useEffect, useMemo, useState } from 'react'
import { useWindowSize } from '@gpuix/react'
import { chatTitle, searchChats, type Chat } from '@messages/core'
import { useAppState } from './use-app-state'
import { previewText } from './sidebar'
import { C, RADIUS, S, TITLEBAR_HEIGHT, TYPE } from './theme'
import { Icon } from './icons'
import { Avatar, overlayShadow } from './primitives'
import { useShell } from './context'

const PANEL_WIDTH = 480
const RESULT_LIMIT = 8
const ROW_HEIGHT = 52

function SwitcherRow({ chat, active, onSelect, onHover }: { chat: Chat; active: boolean; onSelect: () => void; onHover: () => void }) {
  const fg = active ? C.onAccent : C.text
  const muted = active ? C.onAccentSoft : C.secondary
  return (
    <div
      testId={`switcher-row-${chat.identifier}`}
      onClick={onSelect}
      onMouseEnter={onHover}
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: S.x2,
        height: ROW_HEIGHT,
        paddingLeft: S.x2,
        paddingRight: S.x2,
        borderRadius: RADIUS.menuItem,
        cursor: 'pointer',
        flexShrink: 0,
        backgroundColor: active ? C.accent : C.overlay,
      }}
    >
      <Avatar chat={chat} size={32} surface={active ? C.accent : C.overlay} />
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, gap: 1 }}>
        <text style={{ ...TYPE.body, fontWeight: 600, color: fg, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{chatTitle(chat)}</text>
        <text style={{ ...TYPE.preview, color: muted, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{previewText(chat.lastMessage, chat)}</text>
      </div>
    </div>
  )
}

export function Switcher({ onClose }: { onClose: () => void }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const { width } = useWindowSize()
  const [query, setQuery] = useState('')
  const [highlighted, setHighlighted] = useState(0)
  const results = useMemo(() => searchChats(state.chats, query, RESULT_LIMIT), [state.chats, query])

  useEffect(() => setHighlighted(0), [query])

  const open = (chat: Chat) => {
    onClose()
    void shell.store.selectChat(chat.guid)
  }

  return (
    <anchored deferred occlude priority={3} position={{ x: width / 2, y: TITLEBAR_HEIGHT + S.x4 }} anchor="topCenter" fit="snap" snapMargin={S.x2}>
      <div
        testId="switcher"
        onMouseDownOutside={onClose}
        style={{
          display: 'flex',
          flexDirection: 'column',
          width: PANEL_WIDTH,
          borderRadius: RADIUS.menu,
          backgroundColor: C.overlay,
          borderWidth: 1,
          borderColor: C.overlayBorder,
          boxShadow: overlayShadow,
          userSelect: 'none',
        }}
      >
        <div
          style={{
            display: 'flex',
            flexDirection: 'row',
            alignItems: 'center',
            gap: S.x2,
            height: 40,
            paddingLeft: S.x3,
            paddingRight: S.x3,
            borderBottomWidth: 1,
            borderColor: C.separator,
          }}
        >
          <Icon name="search" size={13} color={C.tertiary} />
          <input
            testId="switcher-input"
            autoFocus
            value={query}
            placeholder="Jump to a conversation"
            onChange={(event) => setQuery(event.value ?? '')}
            onSubmit={() => {
              const chat = results[highlighted]
              if (chat) open(chat)
            }}
            onKeyDown={(event) => {
              if (event.key === 'escape') onClose()
              else if (event.key === 'down') setHighlighted((index) => Math.min(index + 1, results.length - 1))
              else if (event.key === 'up') setHighlighted((index) => Math.max(index - 1, 0))
            }}
            theme={{ caret: C.accent, textMuted: C.tertiary }}
            style={{ flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, ...TYPE.body, color: C.text, backgroundColor: C.transparent, borderWidth: 0 }}
          />
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', padding: S.x1 }}>
          {results.length === 0 ? (
            <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', height: ROW_HEIGHT, paddingLeft: S.x2 }}>
              <text style={{ ...TYPE.body, color: C.secondary }}>No matching conversations.</text>
            </div>
          ) : (
            results.map((chat, index) => (
              <SwitcherRow key={chat.guid} chat={chat} active={index === highlighted} onSelect={() => open(chat)} onHover={() => setHighlighted(index)} />
            ))
          )}
        </div>
      </div>
    </anchored>
  )
}
