import { normalizeAddress } from './findmy'
import type { Chat, Contact, FocusStatus, Handle, Message } from './model'

/**
 * chat.db keeps one chat per address, so a person you text on two numbers
 * shows up twice. Here those fold into one conversation: the most recently
 * active chat is the primary that the sidebar lists and replies go to, the
 * others are its members and only lend their messages.
 */
export interface Grouping {
  /** Member guid to primary guid, only for chats that belong to a merged conversation. */
  primaryOf: Record<string, string>
  /** Primary guid to every member guid, itself first. */
  merged: Record<string, string[]>
}

interface GroupedState extends Grouping {
  chats: Chat[]
  focus: Record<string, FocusStatus>
  messages: Record<string, Message[]>
  typing: Record<string, boolean>
  hasOlder: Record<string, boolean>
  loading: Record<string, boolean>
}

export function groupChats(chats: Chat[], contacts: Contact[]): Grouping {
  const owner = new Map<string, string>()
  for (const contact of contacts) for (const address of contact.addresses) owner.set(normalizeAddress(address), contact.id)
  const byKey = new Map<string, Chat[]>()
  for (const chat of chats) {
    if (chat.isGroup || chat.participants.length !== 1) continue
    const raw = chat.participants[0]!.address
    const address = normalizeAddress(raw)
    // Alphanumeric sender ids ("Wise") normalise to nothing; they stay apart.
    const key = address ? (owner.get(address) ?? `address:${address}`) : `id:${raw.toLowerCase()}`
    const list = byKey.get(key)
    if (list) list.push(chat)
    else byKey.set(key, [chat])
  }
  const primaryOf: Record<string, string> = {}
  const merged: Record<string, string[]> = {}
  for (const members of byKey.values()) {
    if (members.length < 2) continue
    members.sort((a, b) => b.lastActivity - a.lastActivity)
    const primary = members[0]!.guid
    merged[primary] = members.map((chat) => chat.guid)
    for (const chat of members) primaryOf[chat.guid] = primary
  }
  return { primaryOf, merged }
}

export function conversationGuid(state: Grouping, guid: string): string {
  return state.primaryOf[guid] ?? guid
}

export function conversationMembers(state: Grouping, guid: string): string[] {
  return state.merged[conversationGuid(state, guid)] ?? [guid]
}

/** A stable key for a conversation: the same whichever member is primary today. */
export function conversationKey(state: Grouping, guid: string): string {
  return [...conversationMembers(state, guid)].sort()[0] ?? guid
}

/** The rows the sidebar shows: every chat that is not folded into another. */
export function conversationChats(state: Pick<GroupedState, 'chats' | 'primaryOf'>): Chat[] {
  return state.chats.filter((chat) => (state.primaryOf[chat.guid] ?? chat.guid) === chat.guid)
}

/** Every loaded message across the members, oldest first. A lone chat's list comes back as is. */
export function conversationMessages(state: Pick<GroupedState, 'messages' | 'primaryOf' | 'merged'>, guid: string): Message[] {
  const members = conversationMembers(state, guid)
  if (members.length === 1) return state.messages[members[0]!] ?? []
  const all: Message[] = []
  for (const member of members) all.push(...(state.messages[member] ?? []))
  return all.sort((a, b) => a.date - b.date)
}

export function conversationUnread(state: Pick<GroupedState, 'chats' | 'primaryOf' | 'merged'>, guid: string): boolean {
  const members = new Set(conversationMembers(state, guid))
  return state.chats.some((chat) => members.has(chat.guid) && chat.unread)
}

export function conversationTyping(state: Pick<GroupedState, 'typing' | 'primaryOf' | 'merged'>, guid: string): boolean {
  return conversationMembers(state, guid).some((member) => state.typing[member])
}

export function conversationHasOlder(state: Pick<GroupedState, 'hasOlder' | 'primaryOf' | 'merged'>, guid: string): boolean {
  return conversationMembers(state, guid).some((member) => state.hasOlder[member] !== false)
}

export function conversationLoading(state: Pick<GroupedState, 'loading' | 'primaryOf' | 'merged'>, guid: string): boolean {
  return conversationMembers(state, guid).some((member) => state.loading[member])
}

/** The addresses the conversation reaches the person on, primary first. */
export function conversationHandles(state: Pick<GroupedState, 'chats' | 'primaryOf' | 'merged'>, guid: string): Handle[] {
  const handles: Handle[] = []
  for (const member of conversationMembers(state, guid)) {
    const chat = state.chats.find((item) => item.guid === member)
    for (const handle of chat?.participants ?? []) if (!handles.some((item) => item.address === handle.address)) handles.push(handle)
  }
  return handles
}

/** The key a Focus answer is kept under: one person, however many numbers they have. */
export function focusKey(address: string): string {
  return normalizeAddress(address) || address.toLowerCase()
}

/** The Focus of the person on the other end. A group, or anyone not asked about yet, is `unknown`. */
export function conversationFocus(state: Pick<GroupedState, 'chats' | 'primaryOf' | 'merged' | 'focus'>, guid: string): FocusStatus {
  const chat = state.chats.find((item) => item.guid === conversationGuid(state, guid))
  if (!chat || chat.isGroup) return 'unknown'
  const address = conversationHandles(state, guid)[0]?.address
  if (!address) return 'unknown'
  return state.focus[focusKey(address)] ?? 'unknown'
}
