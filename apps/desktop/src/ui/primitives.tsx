import type { ReactNode } from 'react'
import { Tooltip, TooltipContent, TooltipTrigger, type StyleDesc } from '@gpuix/react'
import { chatTitle, handleName, type Chat, type Handle } from '@messages/core'
import { initials } from '@messages/core'
import { C, RADIUS, TYPE } from './theme'
import { Icon, type IconName } from './icons'

export function IconButton({
  icon,
  label,
  onClick,
  size = 16,
  color = C.secondary,
  active = false,
  disabled = false,
  testId,
  hit = 28,
}: {
  icon: IconName
  label: string
  onClick?: () => void
  size?: number
  color?: string
  active?: boolean
  disabled?: boolean
  testId?: string
  hit?: number
}) {
  const button = (
    <div
      testId={testId}
      tabIndex={disabled ? undefined : -1}
      onClick={disabled ? undefined : onClick}
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
        hover: disabled ? undefined : { backgroundColor: active ? C.selectedSoft : C.raisedHover },
        active: disabled ? undefined : { opacity: 0.7 },
      }}
    >
      <Icon name={icon} size={size} color={active ? C.accent : color} />
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
  borderRadius: 6,
  paddingLeft: 8,
  paddingRight: 8,
  paddingTop: 4,
  paddingBottom: 4,
  boxShadow: { offsetX: 0, offsetY: 4, blurRadius: 12, spreadRadius: 0, color: '#00000066' },
}

/** Messages uses a neutral gradient monogram for anyone without a photo. Colour would imply meaning it does not have. */
export function Avatar({ handle, chat, size = 36 }: { handle?: Handle; chat?: Chat; size?: number }) {
  if (chat?.isGroup) return <GroupAvatar chat={chat} size={size} />
  const person = handle ?? chat?.participants[0]
  const label = person ? handleName(person) : chat ? chatTitle(chat) : '?'
  const monogram = initials(label)
  if (person?.avatar) {
    return <img src={person.avatar} objectFit="cover" style={{ width: size, height: size, borderRadius: size / 2, flexShrink: 0 }} />
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
        background: { type: 'linear-gradient', angle: 180, stops: [{ color: '#a8a8ae', position: 0 }, { color: '#7c7c82', position: 1 }] },
        userSelect: 'none',
      }}
    >
      {monogram === '#' ? (
        <Icon name="person" size={Math.round(size * 0.55)} color="#ffffff" />
      ) : (
        <text style={{ fontSize: Math.round(size * 0.4), fontWeight: 600, color: '#ffffff', lineHeight: Math.round(size * 0.5) }}>{monogram}</text>
      )}
    </div>
  )
}

function GroupAvatar({ chat, size }: { chat: Chat; size: number }) {
  const [first, second] = chat.participants
  const small = Math.round(size * 0.66)
  return (
    <div style={{ width: size, height: size, position: 'relative', flexShrink: 0 }}>
      <div style={{ position: 'absolute', top: 0, right: 0 }}>
        <Avatar handle={second ?? first} size={small} />
      </div>
      <div
        style={{
          position: 'absolute',
          bottom: 0,
          left: 0,
          width: small + 4,
          height: small + 4,
          borderRadius: (small + 4) / 2,
          backgroundColor: C.sidebar,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <Avatar handle={first} size={small} />
      </div>
    </div>
  )
}

export function Divider({ color = C.separator, inset = 0 }: { color?: string; inset?: number }) {
  return <div style={{ height: 1, backgroundColor: color, marginLeft: inset, flexShrink: 0 }} />
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
        paddingLeft: 14,
        paddingRight: 14,
        height: 30,
        borderRadius: RADIUS.control,
        backgroundColor: fill,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.5 : 1,
        userSelect: 'none',
        hover: disabled ? undefined : { opacity: 0.9 },
        active: disabled ? undefined : { opacity: 0.75 },
      }}
    >
      <text style={{ ...TYPE.body, fontWeight: 600, color }}>{children}</text>
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
        height: 32,
        paddingLeft: 10,
        paddingRight: 10,
        borderRadius: RADIUS.control,
        borderWidth: 1,
        borderColor: C.separator,
        backgroundColor: C.canvas,
        color: secure ? C.canvas : C.text,
        fontSize: 14,
        lineHeight: 20,
      }}
    />
  )
}
