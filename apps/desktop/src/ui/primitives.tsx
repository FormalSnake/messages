import { useState, type ReactNode } from 'react'
import { Tooltip, TooltipContent, TooltipTrigger, type EventPayload, type StyleDesc } from '@gpuix/react'
import { chatTitle, handleName, type Chat, type Handle } from '@messages/core'
import { initials } from '@messages/core'
import { C, RADIUS, S, TYPE } from './theme'
import { Icon, type IconName } from './icons'

/**
 * Always painted, transparent until the row is the keyboard cursor, so taking
 * the cursor never reflows the row.
 *
 * gpuix 0.7 emits no focus or blur event for any element, so a ring cannot be
 * driven by GPUI's own focus. The sidebar tracks its cursor itself and passes
 * it in here.
 */
export function ring(active: boolean, color: string = C.focusRing) {
  return { borderWidth: 2, borderColor: active ? color : C.transparent } as const
}

export function IconButton({
  icon,
  label,
  onClick,
  onAuxClick,
  size = 16,
  color = C.secondary,
  active = false,
  disabled = false,
  testId,
  hit = 28,
  strong = false,
  focusable = true,
}: {
  icon: IconName
  label: string
  /** Carries the click's window coordinates, for a caller that anchors a card off the button. */
  onClick?: (event: EventPayload) => void
  onAuxClick?: (event: { x?: number; y?: number; isRightClick?: boolean }) => void
  size?: number
  color?: string
  active?: boolean
  disabled?: boolean
  testId?: string
  hit?: number
  strong?: boolean
  focusable?: boolean
}) {
  const button = (
    <div
      testId={testId}
      tabIndex={disabled || !focusable ? undefined : 0}
      onClick={disabled ? undefined : onClick}
      onAuxClick={onAuxClick}
      onKeyDown={(event) => {
        if (!disabled && (event.key === 'enter' || event.key === 'space')) onClick?.(event)
      }}
      style={{
        width: hit,
        height: hit,
        borderRadius: RADIUS.control,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        flexShrink: 0,
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.4 : 1,
        backgroundColor: active ? C.selectedSoft : undefined,
        hover: disabled ? undefined : { backgroundColor: active ? C.selectedSoft : C.hoverWash },
        active: disabled ? undefined : { backgroundColor: active ? C.selectedSoft : C.pressWash, opacity: 0.75 },
      }}
    >
      <Icon name={icon} size={size} color={active ? C.accent : color} strong={strong} />
    </div>
  )
  return (
    <Tooltip delayDuration={600}>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent side="bottom" sideOffset={6} style={tooltipStyle()}>
        <text style={{ ...TYPE.caption, color: C.text }}>{label}</text>
      </TooltipContent>
    </Tooltip>
  )
}

export function tooltipStyle(): StyleDesc {
  return {
    backgroundColor: C.overlay,
    borderWidth: 1,
    borderColor: C.overlayBorder,
    borderRadius: RADIUS.control,
    paddingLeft: S.x2,
    paddingRight: S.x2,
    paddingTop: S.x1,
    paddingBottom: S.x1,
    boxShadow: { offsetX: 0, offsetY: 4, blurRadius: 12, spreadRadius: 0, color: '#00000066' },
  }
}

export const overlayShadow = { offsetX: 0, offsetY: 10, blurRadius: 28, spreadRadius: 0, color: '#000000a6' } as const

/**
 * A filled child swallows the click meant for its row in GPUI (see the gpuix
 * rules in CLAUDE.md), so every avatar is inert to the pointer.
 */
const INERT = { pointerEvents: 'none' } as const

/** Below this a monogram is a smudge, so the disc stays plain. */
const MONOGRAM_MIN = 18

/**
 * `borderRadius` alone does not clip an `<img>`'s bitmap in GPUI (only the
 * border/background shape), so the circle needs a wrapper with its own
 * `overflow: 'hidden'` too.
 */
function PhotoAvatar({ src, size }: { src: string; size: number }) {
  // The 1px ring is drawn inside the box, so the bitmap has to sit inside the ring or its far edges get cut.
  const inner = size - 2
  return (
    <div style={{ ...INERT, width: size, height: size, borderRadius: size / 2, flexShrink: 0, overflow: 'hidden', borderWidth: 1, borderColor: '#ffffff1a' }}>
      <img src={src} objectFit="cover" style={{ width: inner, height: inner, borderRadius: inner / 2 }} />
    </div>
  )
}

/** Messages uses a neutral gradient monogram for anyone without a photo. Colour would imply meaning it does not have. */
export function Avatar({ handle, chat, size = 36 }: { handle?: Handle; chat?: Chat; size?: number }) {
  if (chat?.isGroup) {
    if (chat.icon) return <PhotoAvatar src={chat.icon} size={size} />
    return <GroupAvatar chat={chat} size={size} />
  }
  const person = handle ?? chat?.participants[0]
  const label = person ? handleName(person) : chat ? chatTitle(chat) : '?'
  const monogram = initials(label)
  if (person?.avatar) return <PhotoAvatar src={person.avatar} size={size} />
  return (
    <div
      style={{
        ...INERT,
        width: size,
        height: size,
        borderRadius: size / 2,
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: { type: 'linear-gradient', angle: 180, stops: [{ color: '#a2a2a8', position: 0 }, { color: '#78787e', position: 1 }] },
        userSelect: 'none',
      }}
    >
      {size < MONOGRAM_MIN ? null : monogram === '#' ? (
        <Icon name="person" size={Math.round(size * 0.52)} color="#ffffff" strong />
      ) : (
        <text style={{ fontSize: Math.round(size * 0.38), fontWeight: 600, color: '#ffffff', lineHeight: Math.round(size * 0.46) }}>{monogram}</text>
      )}
    </div>
  )
}

/**
 * Messages' own group picture: the first few people clustered inside one
 * circle, so it stays round under a selection ring or an unread badge. Rows
 * rather than absolute positioning: GPUI resolves an absolute child against
 * the window, not this box.
 */
function GroupAvatar({ chat, size }: { chat: Chat; size: number }) {
  const people = chat.participants.slice(0, 4)
  // Sized so the outer row of discs keeps a clear margin inside the circle.
  const small = people.length <= 2 ? Math.round(size * 0.42) : Math.round(size * 0.36)
  const rows = people.length <= 2 ? [people] : people.length === 3 ? [[people[0]!], people.slice(1)] : [people.slice(0, 2), people.slice(2)]
  return (
    <div
      style={{
        ...INERT,
        width: size,
        height: size,
        borderRadius: size / 2,
        flexShrink: 0,
        overflow: 'hidden',
        backgroundColor: C.raisedHover,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 1,
      }}
    >
      {rows.map((row, index) => (
        <div key={index} style={{ display: 'flex', flexDirection: 'row', gap: 1 }}>
          {row.map((person) => (
            <Avatar key={person.address} handle={person} size={small} />
          ))}
        </div>
      ))}
    </div>
  )
}

export function Divider({ color = C.separator, inset = 0 }: { color?: string; inset?: number }) {
  return <div style={{ height: 1, backgroundColor: color, marginLeft: inset, marginRight: inset, flexShrink: 0 }} />
}

/** The small all-caps-weight heading above a group of rows. */
export function SectionLabel({ children, inset = S.x4 }: { children: ReactNode; inset?: number }) {
  return (
    <text style={{ ...TYPE.micro, fontWeight: 600, color: C.tertiary, paddingLeft: inset, paddingRight: inset, paddingBottom: S.x1 }}>{children}</text>
  )
}

export function Button({
  children,
  onClick,
  kind = 'secondary',
  testId,
  disabled = false,
}: {
  children: ReactNode
  onClick?: () => void
  kind?: 'primary' | 'secondary' | 'danger'
  testId?: string
  disabled?: boolean
}) {
  const fill = kind === 'primary' ? C.accent : kind === 'danger' ? C.danger : C.raised
  const color = kind === 'secondary' ? C.text : C.onAccent
  return (
    <div
      testId={testId}
      tabIndex={disabled ? undefined : 0}
      onClick={disabled ? undefined : onClick}
      onKeyDown={(event) => {
        if (!disabled && (event.key === 'enter' || event.key === 'space')) onClick?.()
      }}
      style={{
        paddingLeft: S.x3,
        paddingRight: S.x3,
        height: 30,
        borderRadius: RADIUS.control,
        backgroundColor: fill,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        flexShrink: 0,
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.4 : 1,
        userSelect: 'none',
        hover: disabled ? undefined : { opacity: 0.88 },
        active: disabled ? undefined : { opacity: 0.7 },
      }}
    >
      <text style={{ ...TYPE.body, fontWeight: 600, color, whiteSpace: 'nowrap' }}>{children}</text>
    </div>
  )
}

const BULLET = '•'

/**
 * gpuix's input has no password mode, so a secure field shows bullets and
 * works the edit back into the real value: whatever the user typed sits
 * between the bullets that survived on each side.
 */
export function maskedEdit(current: string, shown: string): string {
  let lead = 0
  while (lead < shown.length && shown[lead] === BULLET) lead += 1
  let trail = 0
  while (trail < shown.length - lead && shown[shown.length - 1 - trail] === BULLET) trail += 1
  lead = Math.min(lead, current.length)
  trail = Math.min(trail, current.length - lead)
  return current.slice(0, lead) + shown.slice(lead, shown.length - trail) + current.slice(current.length - trail)
}

export function TextField({
  value,
  onChange,
  onSubmit,
  placeholder,
  testId,
  autoFocus,
  secure,
  width,
}: {
  value: string
  onChange: (next: string) => void
  onSubmit?: () => void
  placeholder?: string
  testId?: string
  autoFocus?: boolean
  secure?: boolean
  width?: number | string
}) {
  const [revealed, setRevealed] = useState(false)
  const masked = Boolean(secure) && !revealed
  const field = (
    <input
      testId={testId}
      value={masked ? BULLET.repeat(value.length) : value}
      autoFocus={autoFocus}
      placeholder={placeholder}
      readOnly={false}
      onChange={(event) => onChange(masked ? maskedEdit(value, event.value ?? '') : (event.value ?? ''))}
      onSubmit={onSubmit}
      theme={{ caret: C.accent, textMuted: C.tertiary }}
      style={{
        flexGrow: 1,
        flexShrink: 1,
        flexBasis: 0,
        minWidth: 0,
        height: 30,
        paddingLeft: S.x2,
        paddingRight: secure ? 0 : S.x2,
        borderWidth: 0,
        backgroundColor: C.transparent,
        color: C.text,
        ...TYPE.body,
      }}
    />
  )
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        width: width ?? '100%',
        minWidth: 0,
        height: 30,
        borderRadius: RADIUS.control,
        borderWidth: 1,
        borderColor: C.separator,
        backgroundColor: C.canvas,
        paddingRight: secure ? 2 : 0,
      }}
    >
      {field}
      {secure ? (
        <IconButton icon={revealed ? 'eyeOff' : 'eye'} label={revealed ? 'Hide password' : 'Show password'} testId={testId ? `${testId}-reveal` : undefined} size={14} hit={24} onClick={() => setRevealed((shown) => !shown)} />
      ) : null}
    </div>
  )
}
