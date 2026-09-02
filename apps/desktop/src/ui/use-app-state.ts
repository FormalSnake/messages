import { useSyncExternalStore } from 'react'
import type { AppState, MessagesStore } from '@messages/core'

export function useAppState(store: MessagesStore): AppState {
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot)
}
