import { createContext, useContext } from 'react'
import type { MessagesStore } from '@messages/core'
import type { IconName } from './icons'

export type MenuItem =
  | { kind?: 'item'; label: string; icon?: IconName; onSelect: () => void; danger?: boolean; disabled?: boolean }
  | { kind: 'separator' }
  | { kind: 'tapbacks'; messageGuid: string; chatGuid: string }

export interface MenuRequest {
  x: number
  y: number
  items: MenuItem[]
}

export interface Shell {
  store: MessagesStore
  openMenu: (request: MenuRequest) => void
  closeMenu: () => void
  openSettings: () => void
  startNewChat: () => void
  toggleInfo: () => void
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
