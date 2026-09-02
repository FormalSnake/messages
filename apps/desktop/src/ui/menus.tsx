import { useEffect, useRef, useState } from 'react'
import { useGpuix, type PublicInstance } from '@gpuix/react'
import { TAPBACK_GLYPH, type TapbackKind } from '@messages/core'
import { useAppState } from './use-app-state'
import { C, RADIUS, TYPE } from './theme'
import { Icon } from './icons'
import { useShell, type MenuItem, type MenuRequest } from './context'

const TAPBACK_ORDER: Array<Exclude<TapbackKind, 'emoji'>> = ['love', 'like', 'dislike', 'laugh', 'emphasize', 'question']

export function ContextMenu({ request }: { request: MenuRequest }) {
  const shell = useShell()
  const { renderer } = useGpuix()
  const menuRef = useRef<PublicInstance | null>(null)
  const [highlighted, setHighlighted] = useState<number | null>(null)
  const selectable = request.items.map((item, index) => ({ item, index })).filter(({ item }) => item.kind === undefined || item.kind === 'item')

  useEffect(() => {
    if (menuRef.current && renderer?.focusElement) renderer.focusElement(menuRef.current.id)
  }, [renderer])

  const activate = (item: MenuItem) => {
    if (item.kind !== undefined && item.kind !== 'item') return
    if (item.disabled) return
    shell.closeMenu()
    item.onSelect()
  }

  return (
    <anchored deferred occlude priority={2} position={{ x: request.x, y: request.y }} fit="snap" snapMargin={8}>
      <div
        ref={menuRef}
        testId="context-menu"
        tabIndex={-1}
        onMouseDownOutside={shell.closeMenu}
        onKeyDown={(event) => {
          if (event.key === 'escape') shell.closeMenu()
          if (event.key === 'down' || event.key === 'up') {
            const current = highlighted === null ? -1 : selectable.findIndex(({ index }) => index === highlighted)
            const step = event.key === 'down' ? 1 : -1
            const next = selectable[(current + step + selectable.length) % selectable.length]
            if (next) setHighlighted(next.index)
          }
          if (event.key === 'enter' && highlighted !== null) {
            const item = request.items[highlighted]
            if (item) activate(item)
          }
        }}
        style={{
          display: 'flex',
          flexDirection: 'column',
          minWidth: 200,
          paddingTop: 5,
          paddingBottom: 5,
          paddingLeft: 5,
          paddingRight: 5,
          borderRadius: RADIUS.row,
          backgroundColor: C.overlay,
          borderWidth: 1,
          borderColor: C.overlayBorder,
          boxShadow: { offsetX: 0, offsetY: 8, blurRadius: 24, spreadRadius: 0, color: '#00000080' },
          userSelect: 'none',
        }}
      >
        {request.items.map((item, index) => {
          if (item.kind === 'separator') {
            return <div key={index} style={{ height: 1, backgroundColor: C.separator, marginTop: 4, marginBottom: 4, marginLeft: 8, marginRight: 8 }} />
          }
          if (item.kind === 'tapbacks') {
            return <TapbackRow key={index} chatGuid={item.chatGuid} messageGuid={item.messageGuid} />
          }
          const active = highlighted === index
          return (
            <div
              key={index}
              testId={`menu-${item.label.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`}
              onClick={() => activate(item)}
              onMouseEnter={() => setHighlighted(index)}
              style={{
                display: 'flex',
                flexDirection: 'row',
                alignItems: 'center',
                gap: 8,
                height: 28,
                paddingLeft: 10,
                paddingRight: 12,
                borderRadius: 6,
                cursor: item.disabled ? 'default' : 'pointer',
                opacity: item.disabled ? 0.45 : 1,
                backgroundColor: active && !item.disabled ? C.accent : C.overlay,
                hover: item.disabled ? undefined : { backgroundColor: C.accent },
              }}
            >
              {item.icon ? <Icon name={item.icon} size={14} color={item.danger && !active ? C.danger : active ? C.onAccent : C.text} /> : <div style={{ width: 14 }} />}
              <text style={{ ...TYPE.body, color: item.danger && !active ? C.danger : active ? C.onAccent : C.text, whiteSpace: 'nowrap' }}>{item.label}</text>
            </div>
          )
        })}
      </div>
    </anchored>
  )
}

function TapbackRow({ chatGuid, messageGuid }: { chatGuid: string; messageGuid: string }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const message = state.messages[chatGuid]?.find((item) => item.guid === messageGuid)
  const mine = message?.tapbacks.find((item) => item.fromMe)
  return (
    <div
      testId="tapback-row"
      style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 2, paddingLeft: 4, paddingRight: 4, paddingBottom: 4, marginBottom: 2 }}
    >
      {TAPBACK_ORDER.map((kind) => {
        const selected = mine?.kind === kind
        return (
          <div
            key={kind}
            testId={`tapback-${kind}`}
            onClick={() => {
              shell.closeMenu()
              void shell.store.react(chatGuid, messageGuid, kind)
            }}
            style={{
              width: 32,
              height: 32,
              borderRadius: 16,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              cursor: 'pointer',
              backgroundColor: selected ? C.accent : C.overlay,
              hover: { backgroundColor: selected ? C.accent : C.raisedHover },
              active: { opacity: 0.7 },
            }}
          >
            <text style={{ fontSize: 17, lineHeight: 22, color: C.text }}>{TAPBACK_GLYPH[kind]}</text>
          </div>
        )
      })}
    </div>
  )
}
