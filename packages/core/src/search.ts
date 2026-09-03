/**
 * The sidebar search box's query language: free text plus a handful of
 * operators borrowed from Gmail's search syntax (`from:`, `after:`/`before:`
 * are inclusive/exclusive the same way). `parseSearchQuery` is pure and
 * testable on its own; `resolveSearchQuery` turns the raw operator values
 * into the addresses and chat guids `Transport.searchMessages` filters on,
 * which needs the contact list and chat participants to do.
 */
import { chatTitle, type Chat, type Contact } from './model'
import type { SearchFilters } from './transport'

export type AttachmentFilter = NonNullable<SearchFilters['attachments']>

export interface ParsedSearchQuery {
  text: string
  fromMe: boolean
  /** Raw `from:` values, not yet resolved to addresses. */
  senders: string[]
  attachments?: AttachmentFilter
  links: boolean
  /** Epoch ms, local midnight of the given date. Inclusive lower bound. */
  after?: number
  /** Epoch ms, local midnight of the given date. Exclusive upper bound. */
  before?: number
  /** Raw `in:` values, not yet resolved to chat guids. */
  chatNames: string[]
}

const DATE = /^(\d{4})-(\d{2})-(\d{2})$/

function parseLocalDate(value: string): number | undefined {
  const match = DATE.exec(value)
  if (!match) return undefined
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]))
  return Number.isNaN(date.getTime()) ? undefined : date.getTime()
}

/** Splits on whitespace, keeping `key:value` together and letting the value be quoted for a multi-word name. */
function tokenize(raw: string): string[] {
  const tokens: string[] = []
  const pattern = /([a-zA-Z]+):"([^"]*)"|(\S+)/g
  let match: RegExpExecArray | null
  while ((match = pattern.exec(raw))) {
    tokens.push(match[1] !== undefined ? `${match[1]}:${match[2]}` : match[3]!)
  }
  return tokens
}

/**
 * Parses one token at a time. An operator with a value that fails to parse
 * (an unknown `has:`, a malformed date) falls back to plain text instead of
 * silently vanishing.
 */
export function parseSearchQuery(raw: string): ParsedSearchQuery {
  const parsed: ParsedSearchQuery = { text: '', fromMe: false, senders: [], links: false, chatNames: [] }
  const words: string[] = []
  for (const token of tokenize(raw)) {
    const colon = token.indexOf(':')
    const key = colon > 0 ? token.slice(0, colon).toLowerCase() : ''
    const value = colon > 0 ? token.slice(colon + 1) : ''
    if (key === 'from' && value.toLowerCase() === 'me') parsed.fromMe = true
    else if (key === 'from' && value) parsed.senders.push(value)
    else if (key === 'has' && (value === 'photo' || value === 'image')) parsed.attachments = 'image'
    else if (key === 'has' && value === 'video') parsed.attachments = 'video'
    else if (key === 'has' && value === 'file') parsed.attachments = 'file'
    else if (key === 'has' && value === 'link') parsed.links = true
    else if (key === 'before' && parseLocalDate(value) !== undefined) parsed.before = parseLocalDate(value)
    else if (key === 'after' && parseLocalDate(value) !== undefined) parsed.after = parseLocalDate(value)
    else if (key === 'in' && value) parsed.chatNames.push(value)
    else words.push(token)
  }
  parsed.text = words.join(' ')
  return parsed
}

export interface SearchContext {
  contacts: Contact[]
  chats: Chat[]
}

/** A contact whose name contains the token, every one of its addresses. Falls back to the token itself, in case it is already an address. */
function resolveSender(token: string, context: SearchContext): string[] {
  const needle = token.toLowerCase()
  const contactMatches = context.contacts.filter((contact) => contact.name.toLowerCase().includes(needle))
  if (contactMatches.length > 0) return contactMatches.flatMap((contact) => contact.addresses)
  const handleAddresses = new Set<string>()
  for (const chat of context.chats) {
    for (const handle of chat.participants) {
      if (handle.name?.toLowerCase().includes(needle)) handleAddresses.add(handle.address)
    }
  }
  return handleAddresses.size > 0 ? [...handleAddresses] : [token]
}

function resolveChatName(token: string, context: SearchContext): string[] {
  const needle = token.toLowerCase()
  return context.chats.filter((chat) => chatTitle(chat).toLowerCase().includes(needle)).map((chat) => chat.guid)
}

/**
 * Resolves `from:` and `in:` against the address book and every known chat's
 * participants. `in:` with no matching chat still constrains the search (an
 * empty, not absent, `chatGuids`), so a typo returns nothing instead of
 * silently searching every conversation.
 */
export function resolveSearchQuery(parsed: ParsedSearchQuery, context: SearchContext): { text: string; filters: SearchFilters } {
  const filters: SearchFilters = {}
  if (parsed.fromMe) filters.fromMe = true
  if (parsed.senders.length > 0) filters.senders = parsed.senders.flatMap((token) => resolveSender(token, context))
  if (parsed.attachments) filters.attachments = parsed.attachments
  if (parsed.links) filters.links = true
  // SearchFilters.after/before are inclusive on both ends, matching the
  // server's own date query. after:DATE ("on or after that day") maps
  // straight onto it; before:DATE ("before that day") excludes the day
  // itself, so its boundary moves back one millisecond.
  if (parsed.after !== undefined) filters.after = parsed.after
  if (parsed.before !== undefined) filters.before = parsed.before - 1
  if (parsed.chatNames.length > 0) filters.chatGuids = parsed.chatNames.flatMap((token) => resolveChatName(token, context))
  return { text: parsed.text, filters }
}
