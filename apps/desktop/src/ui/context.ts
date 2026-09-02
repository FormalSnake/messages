import { createContext, useContext } from 'react'
import type { MessagesStore } from '@messages/core'
import type { IconName } from './icons'

export type MenuItem =
  | {
      kind?: 'item'
      label: string
      icon?: IconName
      onSelect: () => void
      danger?: boolean
      disabled?: boolean
      /** Right-aligned hint, already formatted for the platform by `shortcut()`. */
      shortcut?: string
    }
  | { kind: 'separator' }
  | { kind: 'header'; label: string }
  | { kind: 'tapbacks'; messageGuid: string; chatGuid: string }

export interface MenuRequest {
  /** Window coordinates of the click, or of the anchor's edge. */
  x: number
  y: number
  items: MenuItem[]
  /**
   * `below` hangs the menu off the point, like every context menu.
   * `above` rests its bottom edge on the point, which is how the tapback
   * picker sits on top of the bubble it belongs to.
   */
  placement?: 'below' | 'above'
  /** Centre the menu on `x` instead of starting there. Used by the picker. */
  align?: 'start' | 'center'
  minWidth?: number
}

export interface Shell {
  store: MessagesStore
  openMenu: (request: MenuRequest) => void
  closeMenu: () => void
  openSettings: () => void
  startNewChat: () => void
  toggleInfo: () => void
  /** Menus that mean "show me the details" must not close a panel that is already open. */
  setInfo: (open: boolean) => void
  focusSearch: () => void
}

export const ShellContext = createContext<Shell | null>(null)

export function useShell(): Shell {
  const shell = useContext(ShellContext)
  if (!shell) throw new Error('useShell needs a ShellContext provider')
  return shell
}

export function useStore(): MessagesStore {
  return useShell().store
}

export function primaryModifier(modifiers: { ctrl?: boolean; cmd?: boolean } | undefined): boolean {
  return Boolean(modifiers?.cmd || modifiers?.ctrl)
}

const darwin = typeof process !== 'undefined' && process.platform === 'darwin'

/** Menu hints read the way the platform writes them: "⇧⌘U" on a Mac, "Ctrl+Shift+U" elsewhere. */
export function shortcut(key: string, options: { shift?: boolean; alt?: boolean } = {}): string {
  if (darwin) return `${options.shift ? '⇧' : ''}${options.alt ? '⌥' : ''}⌘${key.toUpperCase()}`
  return `Ctrl+${options.shift ? 'Shift+' : ''}${options.alt ? 'Alt+' : ''}${key.length === 1 ? key.toUpperCase() : key}`
}
