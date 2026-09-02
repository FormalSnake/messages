import { chatTitle, handleName, type Chat, type Message } from './model'

function summary(message: Message): string {
  if (message.dateRetracted) return 'Unsent a message'
  if (message.attachments.length > 0) {
    const first = message.attachments[0]
    if (first?.mime.startsWith('image/')) return message.text || 'Sent a photo'
    if (first?.mime.startsWith('video/')) return message.text || 'Sent a video'
    if (message.isAudio) return 'Sent an audio message'
    return message.text || `Sent ${first?.name ?? 'an attachment'}`
  }
  return message.text
}

/** Desktop notification through notify-send on Linux and osascript on macOS. Failures are logged, never thrown. */
export async function notifyIncoming(chat: Chat, message: Message): Promise<void> {
  const title = chatTitle(chat)
  const sender = message.sender ? handleName(message.sender) : title
  const body = chat.isGroup && message.sender ? `${sender}: ${summary(message)}` : summary(message)
  try {
    if (process.platform === 'darwin') {
      const script = `display notification ${JSON.stringify(body)} with title ${JSON.stringify(title)}`
      await Bun.spawn(['osascript', '-e', script], { stdout: 'ignore', stderr: 'ignore' }).exited
      return
    }
    await Bun.spawn(['notify-send', '--app-name=Messages', '--category=im.received', title, body], { stdout: 'ignore', stderr: 'ignore' }).exited
  } catch (error) {
    console.error(`notify: ${String(error)}`)
  }
}
