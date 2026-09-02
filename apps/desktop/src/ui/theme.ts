// Apple's dark-appearance system colours: the app should read as Messages, not as a theme of it.
export const C = {
  canvas: '#1c1c1e',
  sidebar: '#232325',
  sidebarBorder: '#2c2c2e',
  raised: '#2c2c2e',
  raisedHover: '#3a3a3c',
  overlay: '#2c2c2e',
  overlayBorder: '#48484a',
  separator: '#38383a',
  text: '#f2f2f7',
  secondary: '#98989f',
  tertiary: '#6e6e73',
  ghost: '#48484a',
  accent: '#0a84ff',
  onAccent: '#ffffff',
  selected: '#0a84ff',
  selectedSoft: '#0a84ff33',
  imessage: '#0a84ff',
  sms: '#30d158',
  received: '#3a3a3c',
  receivedText: '#f2f2f7',
  danger: '#ff453a',
  warning: '#ffd60a',
  online: '#30d158',
  offline: '#ff453a',
  tapback: '#48484a',
  tapbackMine: '#0a84ff',
  unread: '#0a84ff',
  focusRing: '#0a84ff',
} as const

const darwin = typeof process !== 'undefined' && process.platform === 'darwin'

export const FONT_SANS = process.env.MESSAGES_FONT ?? (darwin ? 'SF Pro Text' : 'Noto Sans')

export const TYPE = {
  title: { fontSize: 15, fontWeight: 600, lineHeight: 20 },
  body: { fontSize: 14, lineHeight: 19 },
  bubble: { fontSize: 14.5, lineHeight: 20 },
  caption: { fontSize: 12, lineHeight: 16 },
  micro: { fontSize: 11, lineHeight: 14 },
  large: { fontSize: 22, fontWeight: 700, lineHeight: 28 },
} as const

export const SPACE = 4
export const RADIUS = { bubble: 18, row: 10, control: 8, pill: 999, card: 12 } as const
export const SIDEBAR_WIDTH = 300
export const TITLEBAR_HEIGHT = 52
export const TRAFFIC_LIGHT_CLEARANCE = darwin ? 78 : 0
export const BUBBLE_MAX_WIDTH = 460
