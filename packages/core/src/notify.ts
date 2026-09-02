import { chatTitle, handleName, tapbackGlyph, type Chat, type Message } from './model'

export interface NotifyOptions {
  /** The message a tapback landed on, when it is loaded. */
  target?: Message
  /** Icon path for the Linux notification daemon. */
  icon?: string
}

export type NotifyAction = 'open' | null

function summary(message: Message): string {
  if (message.dateRetracted) return 'Unsent a message'
  if (message.attachments.length > 0) {
    const first = message.attachments.find((item) => !item.hidden) ?? message.attachments[0]
    if (first?.mime.startsWith('image/')) return message.text || (first.isSticker ? 'Sent a sticker' : 'Sent a photo')
    if (first?.mime.startsWith('video/')) return message.text || 'Sent a video'
    if (message.isAudio) return 'Sent an audio message'
    return message.text || `Sent ${first?.name ?? 'an attachment'}`
  }
  return message.text
}

function body(chat: Chat, message: Message, target?: Message): string | null {
  const sender = message.sender ? handleName(message.sender) : chatTitle(chat)
  if (message.reaction) {
    if (message.reaction.removed) return null
    const quoted = target?.text?.trim() ? `“${target.text.trim().slice(0, 80)}”` : target?.attachments.length ? 'an attachment' : 'a message'
    return `${sender} reacted ${tapbackGlyph(message.reaction)} to ${quoted}`
  }
  const text = summary(message)
  return chat.isGroup && message.sender ? `${sender}: ${text}` : text
}

/**
 * Desktop notification: notify-send on Linux (with an Open action when the
 * daemon supports it), osascript on macOS. Resolves to 'open' when the person
 * clicked the action. Failures are logged, never thrown.
 */
export async function notifyIncoming(chat: Chat, message: Message, options: NotifyOptions = {}): Promise<NotifyAction> {
  const title = chatTitle(chat)
  const text = body(chat, message, options.target)
  if (!text) return null
  try {
    if (process.platform === 'darwin') {
      const script = `display notification ${JSON.stringify(text)} with title ${JSON.stringify(title)}`
      await Bun.spawn(['osascript', '-e', script], { stdout: 'ignore', stderr: 'ignore' }).exited
      return null
    }
    const args = ['notify-send', '--app-name=Messages', '--category=im.received', '--action=open=Open']
    if (options.icon) args.push(`--icon=${options.icon}`)
    args.push(title, text)
    const child = Bun.spawn(args, { stdout: 'pipe', stderr: 'ignore' })
    const output = (await new Response(child.stdout).text()).trim()
    await child.exited
    return output === 'open' ? 'open' : null
  } catch (error) {
    console.error(`notify: ${String(error)}`)
    return null
  }
}
