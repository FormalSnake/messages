import { useEffect, useMemo, useRef, useState } from 'react'
import { useGpuix, type PublicInstance } from '@gpuix/react'
import { TAPBACK_GLYPH, type TapbackKind } from '@messages/core'
import { useAppState } from './use-app-state'
import { C, RADIUS, S, TYPE } from './theme'
import { Icon } from './icons'
import { overlayShadow } from './primitives'
import { useShell, type MenuItem, type MenuRequest } from './context'

const TAPBACK_ORDER: Array<Exclude<TapbackKind, 'emoji'>> = ['love', 'like', 'dislike', 'laugh', 'emphasize', 'question']

/** The corner of the menu that lands on the click point. */
function anchorFor(request: MenuRequest): 'topLeft' | 'topCenter' | 'bottomLeft' | 'bottomCenter' {
  const above = request.placement === 'above'
  if (request.align === 'center') return above ? 'bottomCenter' : 'topCenter'
  return above ? 'bottomLeft' : 'topLeft'
}

export function ContextMenu({ request }: { request: MenuRequest }) {
  const shell = useShell()
  const { renderer } = useGpuix()
  const menuRef = useRef<PublicInstance | null>(null)
  const [highlighted, setHighlighted] = useState<number | null>(null)
  const selectable = useMemo(
    () => request.items.map((item, index) => ({ item, index })).filter(({ item }) => item.kind === undefined || item.kind === 'item'),
    [request.items],
  )
  const pickerOnly = request.items.length === 1 && request.items[0]?.kind === 'tapbacks'

  useEffect(() => {
    if (menuRef.current && renderer?.focusElement) renderer.focusElement(menuRef.current.id)
  }, [renderer])

  const activate = (item: MenuItem) => {
    if (item.kind !== undefined && item.kind !== 'item') return
    if (item.disabled) return
    shell.closeMenu()
    item.onSelect()
  }

  const step = (delta: number) => {
    if (selectable.length === 0) return
    const current = highlighted === null ? (delta > 0 ? -1 : 0) : selectable.findIndex(({ index }) => index === highlighted)
    const next = selectable[(current + delta + selectable.length) % selectable.length]
    if (next) setHighlighted(next.index)
  }

  return (
    <anchored deferred occlude priority={2} position={{ x: request.x, y: request.y }} anchor={anchorFor(request)} fit="snap" snapMargin={S.x2}>
      <div
        ref={menuRef}
        testId="context-menu"
        tabIndex={-1}
        onMouseDownOutside={shell.closeMenu}
        onKeyDown={(event) => {
          if (event.key === 'escape') shell.closeMenu()
          else if (event.key === 'down') step(1)
          else if (event.key === 'up') step(-1)
          else if (event.key === 'home') setHighlighted(selectable[0]?.index ?? null)
          else if (event.key === 'end') setHighlighted(selectable[selectable.length - 1]?.index ?? null)
          else if (event.key === 'enter' || event.key === 'space') {
            const item = highlighted === null ? undefined : request.items[highlighted]
            if (item) activate(item)
          }
        }}
        style={{
          display: 'flex',
          flexDirection: 'column',
          minWidth: pickerOnly ? undefined : (request.minWidth ?? 196),
          padding: S.x1,
          borderRadius: pickerOnly ? RADIUS.pill : RADIUS.menu,
          backgroundColor: C.overlay,
          borderWidth: 1,
          borderColor: C.overlayBorder,
          boxShadow: overlayShadow,
          userSelect: 'none',
        }}
      >
        {request.items.map((item, index) => {
          if (item.kind === 'separator') {
            return <div key={index} style={{ height: 1, backgroundColor: C.separator, marginTop: S.x1, marginBottom: S.x1, marginLeft: S.x2, marginRight: S.x2 }} />
          }
          if (item.kind === 'header') {
            return (
              <text key={index} style={{ ...TYPE.micro, fontWeight: 600, color: C.tertiary, paddingLeft: S.x2, paddingRight: S.x2, paddingTop: S.x1, paddingBottom: S.x1 }}>
                {item.label}
              </text>
            )
          }
          if (item.kind === 'tapbacks') {
            return <TapbackRow key={index} chatGuid={item.chatGuid} messageGuid={item.messageGuid} bare={pickerOnly} />
          }
          const active = highlighted === index && !item.disabled
          const fg = item.disabled ? C.secondary : item.danger && !active ? C.danger : active ? C.onAccent : C.text
          return (
            <div
              key={index}
              testId={`menu-${item.label.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`}
              onClick={() => activate(item)}
              onMouseEnter={() => setHighlighted(index)}
              onMouseLeave={() => setHighlighted((current) => (current === index ? null : current))}
              style={{
                display: 'flex',
                flexDirection: 'row',
                alignItems: 'center',
                gap: S.x2,
                height: 28,
                paddingLeft: S.x2,
                paddingRight: S.x2,
                borderRadius: RADIUS.menuItem,
                cursor: item.disabled ? 'default' : 'pointer',
                opacity: item.disabled ? 0.4 : 1,
                backgroundColor: active ? C.accent : C.overlay,
              }}
            >
              <div style={{ width: 14, display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
                {item.icon ? <Icon name={item.icon} size={14} color={fg} /> : null}
              </div>
              <text style={{ ...TYPE.body, color: fg, flexGrow: 1, minWidth: 0, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{item.label}</text>
              {item.shortcut ? (
                <text style={{ ...TYPE.body, color: active ? C.onAccentSoft : C.tertiary, whiteSpace: 'nowrap', paddingLeft: S.x3 }}>{item.shortcut}</text>
              ) : null}
            </div>
          )
        })}
      </div>
    </anchored>
  )
}

function TapbackRow({ chatGuid, messageGuid, bare }: { chatGuid: string; messageGuid: string; bare: boolean }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const message = state.messages[chatGuid]?.find((item) => item.guid === messageGuid)
  const mine = message?.tapbacks.find((item) => item.fromMe)
  return (
    <div
      testId="tapback-row"
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: S.x1,
        paddingLeft: S.x1,
        paddingRight: S.x1,
        paddingBottom: bare ? 0 : S.x1,
        marginBottom: bare ? 0 : S.x1,
        borderBottomWidth: bare ? 0 : 1,
        borderColor: C.separator,
      }}
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
              width: 30,
              height: 30,
              borderRadius: 15,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
              cursor: 'pointer',
              backgroundColor: selected ? C.accent : C.overlay,
              hover: { backgroundColor: selected ? C.accent : C.hoverWash },
              active: { backgroundColor: selected ? C.accent : C.pressWash, opacity: 0.75 },
            }}
          >
            <text style={{ fontSize: 16, lineHeight: 20, color: C.text }}>{TAPBACK_GLYPH[kind]}</text>
          </div>
        )
      })}
    </div>
  )
}
