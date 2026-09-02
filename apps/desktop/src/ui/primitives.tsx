import type { ReactNode } from 'react'
import { Tooltip, TooltipContent, TooltipTrigger, type StyleDesc } from '@gpuix/react'
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
  onClick?: () => void
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
        if (!disabled && (event.key === 'enter' || event.key === 'space')) onClick?.()
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
      <TooltipContent side="bottom" sideOffset={6} style={tooltipStyle}>
        <text style={{ ...TYPE.caption, color: C.text }}>{label}</text>
      </TooltipContent>
    </Tooltip>
  )
}

export const tooltipStyle: StyleDesc = {
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

export const overlayShadow = { offsetX: 0, offsetY: 10, blurRadius: 28, spreadRadius: 0, color: '#000000a6' } as const

/** Messages uses a neutral gradient monogram for anyone without a photo. Colour would imply meaning it does not have. */
export function Avatar({ handle, chat, size = 36, surface = C.sidebar }: { handle?: Handle; chat?: Chat; size?: number; surface?: string }) {
  if (chat?.isGroup) return <GroupAvatar chat={chat} size={size} surface={surface} />
  const person = handle ?? chat?.participants[0]
  const label = person ? handleName(person) : chat ? chatTitle(chat) : '?'
  const monogram = initials(label)
  if (person?.avatar) {
    return (
      <img
        src={person.avatar}
        objectFit="cover"
        style={{ width: size, height: size, borderRadius: size / 2, flexShrink: 0, borderWidth: 1, borderColor: '#ffffff1a' }}
      />
    )
  }
  return (
    <div
      style={{
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
      {monogram === '#' ? (
        <Icon name="person" size={Math.round(size * 0.52)} color="#ffffff" strong />
      ) : (
        <text style={{ fontSize: Math.round(size * 0.38), fontWeight: 600, color: '#ffffff', lineHeight: Math.round(size * 0.46) }}>{monogram}</text>
      )}
    </div>
  )
}

/**
 * Two circles, the newer participant in front and ringed in the surface colour
 * so the overlap reads as depth. Laid out with a negative margin rather than
 * absolute positioning: GPUI resolves an absolute child against the window, not
 * this box, which threw the second avatar into the row above.
 */
function GroupAvatar({ chat, size, surface }: { chat: Chat; size: number; surface: string }) {
  const [first, second] = chat.participants
  const back = Math.round(size * 0.5)
  const ring = 2
  const front = Math.round(size * 0.64)
  const frontBox = front + ring * 2
  return (
    <div style={{ width: size, height: size, flexShrink: 0, display: 'flex', flexDirection: 'column', alignItems: 'flex-start' }}>
      <div style={{ flexShrink: 0 }}>
        <Avatar handle={second ?? first} size={back} surface={surface} />
      </div>
      <div
        style={{
          width: frontBox,
          height: frontBox,
          marginLeft: size - frontBox,
          marginTop: size - frontBox - back,
          borderRadius: frontBox / 2,
          backgroundColor: surface,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          flexShrink: 0,
        }}
      >
        <Avatar handle={first} size={front} surface={surface} />
      </div>
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
  return (
    <input
      testId={testId}
      value={value}
      autoFocus={autoFocus}
      placeholder={placeholder}
      readOnly={false}
      onChange={(event) => onChange(event.value ?? '')}
      onSubmit={onSubmit}
      theme={{ caret: C.accent, textMuted: C.tertiary }}
      style={{
        width: width ?? '100%',
        minWidth: 0,
        height: 30,
        paddingLeft: S.x2,
        paddingRight: S.x2,
        borderRadius: RADIUS.control,
        borderWidth: 1,
        borderColor: C.separator,
        backgroundColor: C.canvas,
        color: secure ? C.canvas : C.text,
        ...TYPE.body,
      }}
    />
  )
}
