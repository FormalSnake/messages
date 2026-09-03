import { chatTitle, handleName, type Chat } from './model'

const WORD_START_BONUS = 8
const CONSECUTIVE_BONUS = 6
/** Keeps a long haystack from beating a short one on an otherwise equal match. */
const LENGTH_PENALTY = 0.15

/**
 * Subsequence match: every character of `query` must appear in `text` in
 * order, not necessarily adjacent. Returns null when it does not match, else
 * a score where consecutive runs and matches at a word start score higher,
 * and a shorter `text` scores higher than a longer one for the same hits.
 */
export function fuzzyScore(query: string, text: string): number | null {
  const needle = query.trim().toLowerCase()
  if (!needle) return 0
  const haystack = text.toLowerCase()
  let cursor = 0
  let score = 0
  let run = 0
  for (const char of needle) {
    const found = haystack.indexOf(char, cursor)
    if (found === -1) return null
    const previous = haystack[found - 1]
    const atWordStart = found === 0 || previous === ' ' || previous === '-' || previous === '@' || previous === '.'
    run = found === cursor ? run + 1 : 1
    score += 1 + run * CONSECUTIVE_BONUS + (atWordStart ? WORD_START_BONUS : 0)
    cursor = found + 1
  }
  return score - haystack.length * LENGTH_PENALTY
}

/** The best score for `query` across several fields naming the same thing, or null if none match. */
function bestScore(query: string, candidates: string[]): number | null {
  let best: number | null = null
  for (const candidate of candidates) {
    if (!candidate) continue
    const score = fuzzyScore(query, candidate)
    if (score !== null && (best === null || score > best)) best = score
  }
  return best
}

/** Chats ranked for the Ctrl+K switcher: title, participant names and addresses, best field wins. */
export function searchChats(chats: Chat[], query: string, limit = 8): Chat[] {
  const needle = query.trim()
  if (!needle) return chats.slice(0, limit)
  const scored: Array<{ chat: Chat; score: number }> = []
  for (const chat of chats) {
    const candidates = [chatTitle(chat), ...chat.participants.flatMap((p) => [handleName(p), p.address])]
    const score = bestScore(needle, candidates)
    if (score !== null) scored.push({ chat, score })
  }
  scored.sort((a, b) => b.score - a.score)
  return scored.slice(0, limit).map((entry) => entry.chat)
}
