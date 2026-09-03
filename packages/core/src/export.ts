import { mkdir } from 'node:fs/promises'
import { homedir } from 'node:os'
import path from 'node:path'
import { hhmm } from './format'
import { chatTitle, handleName, tapbackGlyph, type Chat, type GroupEvent, type Handle, type Message, type Tapback } from './model'

function dayHeading(ms: number): string {
  return new Date(ms).toLocaleDateString(undefined, { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' })
}

function groupEventText(event: GroupEvent): string {
  switch (event.kind) {
    case 'rename':
      return `named the conversation "${event.title}"`
    case 'join':
      return `added ${event.who ? handleName(event.who) : 'someone'}`
    case 'leave':
      return event.who ? `removed ${handleName(event.who)}` : 'left the conversation'
    case 'photo':
      return 'changed the group photo'
  }
}

function attachmentsText(message: Message): string {
  const visible = message.attachments.filter((item) => !item.hidden)
  return visible.map((item) => `[${item.name}]`).join(' ')
}

function tapbackText(tapbacks: Tapback[]): string {
  const counts = new Map<string, number>()
  for (const tapback of tapbacks) counts.set(tapbackGlyph(tapback), (counts.get(tapbackGlyph(tapback)) ?? 0) + 1)
  if (counts.size === 0) return ''
  return `(${[...counts.entries()].map(([glyph, count]) => `${glyph} ${count}`).join(', ')})`
}

function messageLine(message: Message): string {
  const who = message.fromMe ? 'You' : message.sender ? handleName(message.sender) : 'Unknown'
  const body = message.dateRetracted ? 'unsent a message' : message.groupEvent ? groupEventText(message.groupEvent) : message.text
  const parts = [body, attachmentsText(message), tapbackText(message.tapbacks)].filter(Boolean)
  return `**${who}** ${hhmm(message.date)}  ${parts.join(' ')}`.trimEnd()
}

/** Pure formatter: a heading with the participants, a day separator whenever the date changes, then one line per message. */
export function formatConversationMarkdown(chat: Chat, handles: Handle[], messages: Message[]): string {
  const participants = chat.isGroup ? handles.map(handleName).join(', ') : (handles[0] ? handleName(handles[0]) : chat.identifier)
  const lines: string[] = [`# ${participants}`, '']
  let lastDay = ''
  for (const message of messages) {
    const day = dayHeading(message.date)
    if (day !== lastDay) {
      lines.push(`## ${day}`, '')
      lastDay = day
    }
    lines.push(messageLine(message))
  }
  return `${lines.join('\n')}\n`
}

function sanitizeFilename(name: string): string {
  return name.replace(/[/\\:*?"<>|]/g, '-').trim()
}

export function exportFilePath(chat: Chat, now = new Date()): string {
  const date = now.toISOString().slice(0, 10)
  return path.join(homedir(), 'Downloads', `${sanitizeFilename(chatTitle(chat))} ${date}.md`)
}

export async function writeConversationExport(chat: Chat, handles: Handle[], messages: Message[]): Promise<string> {
  const filePath = exportFilePath(chat)
  await mkdir(path.dirname(filePath), { recursive: true })
  await Bun.write(filePath, formatConversationMarkdown(chat, handles, messages))
  return filePath
}
