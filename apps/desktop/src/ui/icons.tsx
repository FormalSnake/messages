import search from 'lucide-static/icons/search.svg' with { type: 'text' }
import squarePen from 'lucide-static/icons/square-pen.svg' with { type: 'text' }
import plus from 'lucide-static/icons/plus.svg' with { type: 'text' }
import arrowUp from 'lucide-static/icons/arrow-up.svg' with { type: 'text' }
import info from 'lucide-static/icons/info.svg' with { type: 'text' }
import video from 'lucide-static/icons/video.svg' with { type: 'text' }
import phone from 'lucide-static/icons/phone.svg' with { type: 'text' }
import chevronLeft from 'lucide-static/icons/chevron-left.svg' with { type: 'text' }
import chevronRight from 'lucide-static/icons/chevron-right.svg' with { type: 'text' }
import chevronDown from 'lucide-static/icons/chevron-down.svg' with { type: 'text' }
import image from 'lucide-static/icons/image.svg' with { type: 'text' }
import paperclip from 'lucide-static/icons/paperclip.svg' with { type: 'text' }
import x from 'lucide-static/icons/x.svg' with { type: 'text' }
import check from 'lucide-static/icons/check.svg' with { type: 'text' }
import reply from 'lucide-static/icons/reply.svg' with { type: 'text' }
import pencil from 'lucide-static/icons/pencil.svg' with { type: 'text' }
import trash from 'lucide-static/icons/trash-2.svg' with { type: 'text' }
import copy from 'lucide-static/icons/copy.svg' with { type: 'text' }
import more from 'lucide-static/icons/ellipsis.svg' with { type: 'text' }
import pin from 'lucide-static/icons/pin.svg' with { type: 'text' }
import pinOff from 'lucide-static/icons/pin-off.svg' with { type: 'text' }
import bellOff from 'lucide-static/icons/bell-off.svg' with { type: 'text' }
import bell from 'lucide-static/icons/bell.svg' with { type: 'text' }
import settings from 'lucide-static/icons/settings.svg' with { type: 'text' }
import wifi from 'lucide-static/icons/wifi.svg' with { type: 'text' }
import wifiOff from 'lucide-static/icons/wifi-off.svg' with { type: 'text' }
import refresh from 'lucide-static/icons/refresh-cw.svg' with { type: 'text' }
import alert from 'lucide-static/icons/circle-alert.svg' with { type: 'text' }
import users from 'lucide-static/icons/users.svg' with { type: 'text' }
import user from 'lucide-static/icons/user.svg' with { type: 'text' }
import mailOpen from 'lucide-static/icons/mail-open.svg' with { type: 'text' }
import mail from 'lucide-static/icons/mail.svg' with { type: 'text' }
import sparkles from 'lucide-static/icons/sparkles.svg' with { type: 'text' }
import smile from 'lucide-static/icons/smile.svg' with { type: 'text' }
import logOut from 'lucide-static/icons/log-out.svg' with { type: 'text' }
import file from 'lucide-static/icons/file.svg' with { type: 'text' }
import mic from 'lucide-static/icons/mic.svg' with { type: 'text' }
import lock from 'lucide-static/icons/lock.svg' with { type: 'text' }
import unlock from 'lucide-static/icons/lock-open.svg' with { type: 'text' }
import externalLink from 'lucide-static/icons/external-link.svg' with { type: 'text' }
import download from 'lucide-static/icons/download.svg' with { type: 'text' }
import messageSquare from 'lucide-static/icons/message-square.svg' with { type: 'text' }
import userMinus from 'lucide-static/icons/user-minus.svg' with { type: 'text' }
import smilePlus from 'lucide-static/icons/smile-plus.svg' with { type: 'text' }
import { C } from './theme'

/**
 * GPUI tints the icon as a mask and does not resolve `currentColor`, so bake a
 * paint colour in. The stroke goes with it: lucide ships at 2, which is the
 * weight for semibold text, and almost every icon here sits beside 13px
 * regular copy where 1.5 is the matching weight.
 */
function bake(source: string, stroke = 1.5): string {
  return source.replace(/currentColor/g, '#000').replace(/stroke-width="2"/g, `stroke-width="${stroke}"`)
}

const SOURCES = {
  search,
  compose: squarePen,
  plus,
  send: arrowUp,
  info,
  video,
  phone,
  chevronLeft,
  chevronRight,
  chevronDown,
  image,
  paperclip,
  close: x,
  check,
  reply,
  edit: pencil,
  trash,
  copy,
  more,
  pin,
  pinOff,
  mute: bellOff,
  unmute: bell,
  settings,
  online: wifi,
  offline: wifiOff,
  refresh,
  alert,
  group: users,
  person: user,
  markRead: mailOpen,
  markUnread: mail,
  effect: sparkles,
  tapback: smile,
  addTapback: smilePlus,
  leave: logOut,
  file,
  audio: mic,
  lock,
  unlock,
  open: externalLink,
  download,
  conversation: messageSquare,
  removePerson: userMinus,
} as const

export type IconName = keyof typeof SOURCES

function baked(stroke: number): Record<IconName, string> {
  const out = {} as Record<IconName, string>
  for (const [name, source] of Object.entries(SOURCES)) out[name as IconName] = bake(source, stroke)
  return out
}

export const ICONS = baked(1.5)
const ICONS_BOLD = baked(2)

/**
 * `strong` matches the 2px stroke to semibold or larger text; the default 1.5
 * sits beside regular copy without out-weighing it.
 */
export function Icon({
  name,
  size = 16,
  color = C.secondary,
  strong = false,
  opacity,
}: {
  name: IconName
  size?: number
  color?: string
  strong?: boolean
  opacity?: number
}) {
  return <svg source={(strong ? ICONS_BOLD : ICONS)[name]} style={{ width: size, height: size, flexShrink: 0, color, opacity }} />
}
