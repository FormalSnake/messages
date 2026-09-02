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
  onAccentSoft: '#ffffffb8',
  selected: '#0a84ff',
  selectedSoft: '#0a84ff33',
  imessage: '#0a84ff',
  sms: '#30d158',
  received: '#3a3a3c',
  receivedText: '#f2f2f7',
  danger: '#ff453a',
  dangerSoft: '#ff453a26',
  warning: '#ffd60a',
  online: '#30d158',
  offline: '#ff453a',
  tapback: '#48484a',
  tapbackMine: '#0a84ff',
  unread: '#0a84ff',
  focusRing: '#0a84ff',
  /** Washes for hover and press over a dark surface, so one value works on any fill. */
  hoverWash: '#ffffff14',
  pressWash: '#ffffff26',
  transparent: '#00000000',
} as const

const darwin = typeof process !== 'undefined' && process.platform === 'darwin'

export const FONT_SANS = process.env.MESSAGES_FONT ?? (darwin ? 'SF Pro Text' : 'Noto Sans')

/**
 * One scale, every gap and inset is a step on it. `S.x1` is 4px.
 * Nothing in the UI should use a spacing number that is not from here.
 */
export const S = {
  x1: 4,
  x2: 8,
  x3: 12,
  x4: 16,
  x5: 20,
  x6: 24,
  x8: 32,
  x10: 40,
} as const

export const TYPE = {
  /** Screen headings: the connect card, the details name. */
  large: { fontSize: 20, fontWeight: 700, lineHeight: 26 },
  /** Titlebar titles. */
  title: { fontSize: 14, fontWeight: 600, lineHeight: 18 },
  /** Every list row, menu item and button label. macOS control size. */
  body: { fontSize: 13, lineHeight: 18 },
  /** The sidebar's two-line message preview. */
  preview: { fontSize: 12.5, lineHeight: 16 },
  /** Bubble copy: the one place that reads as content, not as chrome. */
  bubble: { fontSize: 14.5, lineHeight: 20 },
  caption: { fontSize: 12, lineHeight: 16 },
  micro: { fontSize: 11, lineHeight: 14 },
} as const

/**
 * Radii are concentric: an inner radius plus the padding around it equals the
 * outer one. Menu 10 = item 6 + 4 padding, card 12 = control 6 + 6, and the
 * bubble keeps Messages' own 18.
 */
export const RADIUS = {
  bubble: 18,
  /** The clipped corner inside a run of bubbles from the same sender. */
  bubbleTight: 5,
  row: 8,
  control: 6,
  card: 12,
  menu: 10,
  menuItem: 6,
  pill: 999,
} as const

export const SIDEBAR_WIDTH = 300
/** Below this the sidebar would leave the thread too narrow to read. */
export const SIDEBAR_WIDTH_COMPACT = 248
export const INFO_WIDTH = 280
export const TITLEBAR_HEIGHT = 52
export const TRAFFIC_LIGHT_CLEARANCE = darwin ? 78 : 0
export const ROW_HEIGHT = 64
export const AVATAR_ROW = 44

/** Messages caps a bubble near 62% of the thread, then the measure caps it again. */
export const BUBBLE_MAX_FRACTION = '62%'
export const BUBBLE_MAX_WIDTH = 460
/** Gutter each side of the thread. Bubbles, separators and receipts share it. */
export const THREAD_INSET = 16

export function focusRing(focused: boolean, radius: number) {
  return focused
    ? { borderWidth: 2, borderColor: C.focusRing, borderRadius: radius }
    : { borderWidth: 2, borderColor: C.transparent, borderRadius: radius }
}
