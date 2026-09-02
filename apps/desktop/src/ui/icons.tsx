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
import { C } from './theme'

// GPUI tints the icon as a mask, but it does not resolve `currentColor`, so bake a paint colour in.
function bake(source: string): string {
  return source.replace(/currentColor/g, '#000')
}

export const ICONS = {
  search: bake(search),
  compose: bake(squarePen),
  plus: bake(plus),
  send: bake(arrowUp),
  info: bake(info),
  video: bake(video),
  phone: bake(phone),
  chevronLeft: bake(chevronLeft),
  chevronRight: bake(chevronRight),
  chevronDown: bake(chevronDown),
  image: bake(image),
  paperclip: bake(paperclip),
  close: bake(x),
  check: bake(check),
  reply: bake(reply),
  edit: bake(pencil),
  trash: bake(trash),
  copy: bake(copy),
  more: bake(more),
  pin: bake(pin),
  pinOff: bake(pinOff),
  mute: bake(bellOff),
  unmute: bake(bell),
  settings: bake(settings),
  online: bake(wifi),
  offline: bake(wifiOff),
  refresh: bake(refresh),
  alert: bake(alert),
  group: bake(users),
  person: bake(user),
  markRead: bake(mailOpen),
  markUnread: bake(mail),
  effect: bake(sparkles),
  tapback: bake(smile),
  leave: bake(logOut),
  file: bake(file),
  audio: bake(mic),
  lock: bake(lock),
  unlock: bake(unlock),
} as const

export type IconName = keyof typeof ICONS

export function Icon({ name, size = 16, color = C.secondary }: { name: IconName; size?: number; color?: string }) {
  return <svg source={ICONS[name]} style={{ width: size, height: size, flexShrink: 0, color }} />
}
