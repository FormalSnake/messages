import type { Attachment, Chat, Contact, Handle, Message, ServerInfo, Service, TapbackKind } from './model'
import type { Page, SendAttachmentOptions, SendTextOptions, Transport, TransportEvent } from './transport'

const MIN = 60_000
const HOUR = 60 * MIN
const DAY = 24 * HOUR

const people = {
  alex: { address: '+14155550134', service: 'iMessage', name: 'Alex Rivera' },
  priya: { address: 'priya.natarajan@icloud.com', service: 'iMessage', name: 'Priya Natarajan' },
  jordan: { address: '+14155550188', service: 'SMS', name: 'Jordan Lee' },
  mom: { address: '+14155550101', service: 'iMessage', name: 'Mom' },
  dad: { address: '+14155550102', service: 'iMessage', name: 'Dad' },
  sam: { address: '+14155550103', service: 'iMessage', name: 'Sam' },
  nadia: { address: '+34612345678', service: 'iMessage', name: 'Nadia Haddad' },
  ben: { address: '+14155550170', service: 'iMessage', name: 'Ben Okafor' },
  chloe: { address: 'chloe@martin.design', service: 'iMessage', name: 'Chloe Martin' },
  unknown: { address: '+14155550199', service: 'iMessage' },
} satisfies Record<string, Handle>

function photo(hueA: number, hueB: number, label: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="800" viewBox="0 0 1200 800"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="hsl(${hueA} 70% 55%)"/><stop offset="1" stop-color="hsl(${hueB} 60% 25%)"/></linearGradient></defs><rect width="1200" height="800" fill="url(#g)"/><circle cx="880" cy="250" r="110" fill="hsl(45 95% 80%)" opacity="0.9"/><path d="M0 620 L260 470 L420 560 L640 400 L860 540 L1200 430 L1200 800 L0 800Z" fill="hsl(${hueB} 50% 16%)"/><text x="40" y="760" font-family="sans-serif" font-size="36" fill="white" opacity="0.6">${label}</text></svg>`
  return `data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}`
}

const photos: Record<string, string> = {
  'demo-att-1': photo(200, 260, 'Point Reyes'),
  'demo-att-2': photo(20, 340, 'Golden hour'),
  'demo-att-3': photo(140, 200, 'Trailhead'),
}

function attachment(guid: string, name: string, width: number, height: number): Attachment {
  return { guid, name, mime: 'image/svg+xml', bytes: 812_344, width, height, isSticker: false, localPath: photos[guid] }
}

interface Seed {
  guid: string
  identifier: string
  service: Service
  isGroup: boolean
  displayName?: string
  participants: Handle[]
  messages: Array<Partial<Message> & { text: string; ago: number; from?: Handle }>
}

let seq = 0
function guid(): string {
  seq += 1
  return `demo-msg-${seq.toString().padStart(4, '0')}`
}

const now = Date.now()

const seeds: Seed[] = [
  {
    guid: 'iMessage;-;+14155550134',
    identifier: '+14155550134',
    service: 'iMessage',
    isGroup: false,
    participants: [people.alex],
    messages: [
      { text: 'did you end up trying the vulkan build on the thinkpad?', ago: 3 * HOUR + 22 * MIN, from: people.alex },
      { text: 'Yeah, it boots. Scrolling a 10k row list at 120 fps on integrated graphics.', ago: 3 * HOUR + 20 * MIN },
      { text: 'ok that is genuinely wild', ago: 3 * HOUR + 19 * MIN, from: people.alex },
      { text: 'Tapbacks work too, react to this one', ago: 3 * HOUR + 18 * MIN, tapbacks: [{ guid: 'demo-tb-1', kind: 'laugh', fromMe: false, sender: people.alex }] },
      { text: 'coffee at 4? the place on valencia', ago: 41 * MIN, from: people.alex },
      { text: 'Works for me. Grabbing the laptop.', ago: 39 * MIN, dateDelivered: now - 39 * MIN, dateRead: now - 38 * MIN },
      { text: 'bring the charger this time 🔌', ago: 12 * MIN, from: people.alex, replyTo: 'demo-msg-0006' },
    ],
  },
  {
    guid: 'iMessage;+;chat240119384759',
    identifier: 'chat240119384759',
    service: 'iMessage',
    isGroup: true,
    displayName: 'Family',
    participants: [people.mom, people.dad, people.sam],
    messages: [
      { text: '', ago: 2 * DAY + 5 * HOUR, from: people.mom, groupEvent: { kind: 'rename', title: 'Family' } },
      { text: 'Sunday lunch is at ours, 1pm. Bring the good bread.', ago: 2 * DAY + 4 * HOUR, from: people.mom },
      { text: 'On it 🥖', ago: 2 * DAY + 3 * HOUR + 50 * MIN, tapbacks: [{ guid: 'demo-tb-2', kind: 'love', fromMe: false, sender: people.mom }, { guid: 'demo-tb-3', kind: 'like', fromMe: false, sender: people.dad }] },
      { text: 'Look at this from the hike', ago: DAY + 2 * HOUR, from: people.sam, attachments: [attachment('demo-att-1', 'IMG_4021.HEIC', 1200, 800)] },
      { text: 'Gorgeous!! Which trail?', ago: DAY + HOUR + 55 * MIN, from: people.mom },
      { text: 'Tomales Point, we saw elk', ago: DAY + HOUR + 40 * MIN, from: people.sam },
      { text: 'Can someone grab dad from the airport friday', ago: 5 * HOUR, from: people.sam },
      { text: 'I can. Flight number?', ago: 4 * HOUR + 58 * MIN, dateDelivered: now - 4 * HOUR + 57 * MIN },
    ],
  },
  {
    guid: 'iMessage;-;priya.natarajan@icloud.com',
    identifier: 'priya.natarajan@icloud.com',
    service: 'iMessage',
    isGroup: false,
    participants: [people.priya],
    messages: [
      { text: 'Sent the deck over, let me know what you think about slide 9', ago: DAY + 6 * HOUR, from: people.priya },
      { text: 'Slide 9 is the strongest one. Lead with it.', ago: DAY + 5 * HOUR + 30 * MIN, dateDelivered: now - DAY, dateRead: now - DAY },
      { text: 'Reordered. Also fixed the typo you did not mention 😅', ago: 20 * HOUR, from: people.priya, dateEdited: now - 19 * HOUR },
    ],
  },
  {
    guid: 'SMS;-;+14155550188',
    identifier: '+14155550188',
    service: 'SMS',
    isGroup: false,
    participants: [people.jordan],
    messages: [
      { text: 'Hey its Jordan from the climbing gym, still up for thursday?', ago: 2 * DAY + 3 * HOUR, from: people.jordan },
      { text: 'Yes! 7pm at Dogpatch?', ago: 2 * DAY + 2 * HOUR + 45 * MIN, dateDelivered: now - 2 * DAY },
      { text: 'Perfect see you there', ago: 2 * DAY + 2 * HOUR + 30 * MIN, from: people.jordan },
    ],
  },
  {
    guid: 'iMessage;+;chat881204957120',
    identifier: 'chat881204957120',
    service: 'iMessage',
    isGroup: true,
    displayName: 'Design crit',
    participants: [people.chloe, people.ben, people.nadia],
    messages: [
      { text: 'New composer states, tear it apart', ago: 6 * DAY, from: people.chloe, attachments: [attachment('demo-att-2', 'composer-states.png', 1200, 800)] },
      { text: 'The send button reads as disabled even when it is not. Try the filled circle.', ago: 6 * DAY - 20 * MIN, from: people.ben },
      { text: 'Agree with Ben. Also the placeholder contrast is under 3:1.', ago: 6 * DAY - 15 * MIN },
      { text: 'fixed both, thanks 🙏', ago: 5 * DAY, from: people.chloe, tapbacks: [{ guid: 'demo-tb-4', kind: 'like', fromMe: true }] },
    ],
  },
  {
    guid: 'iMessage;-;+14155550199',
    identifier: '+14155550199',
    service: 'iMessage',
    isGroup: false,
    participants: [people.unknown],
    messages: [{ text: 'Hi, this is Morgan from the bike shop. Your wheel is ready for pickup.', ago: 26 * MIN, from: people.unknown }],
  },
  {
    guid: 'iMessage;-;+34612345678',
    identifier: '+34612345678',
    service: 'iMessage',
    isGroup: false,
    participants: [people.nadia],
    messages: [
      { text: 'Landed in Madrid. It is 38 degrees.', ago: 3 * DAY + 4 * HOUR, from: people.nadia, attachments: [attachment('demo-att-3', 'IMG_0911.HEIC', 1200, 800)] },
      { text: 'Drink water. Send tapas.', ago: 3 * DAY + 3 * HOUR, dateDelivered: now - 3 * DAY, dateRead: now - 3 * DAY },
    ],
  },
  {
    guid: 'iMessage;-;+14155550170',
    identifier: '+14155550170',
    service: 'iMessage',
    isGroup: false,
    participants: [people.ben],
    messages: [
      { text: 'PR is up, no rush', ago: 4 * DAY + 2 * HOUR, from: people.ben },
      { text: 'Reviewing tonight', ago: 4 * DAY + HOUR, dateDelivered: now - 4 * DAY },
    ],
  },
  {
    guid: 'iMessage;-;chloe@martin.design',
    identifier: 'chloe@martin.design',
    service: 'iMessage',
    isGroup: false,
    participants: [people.chloe],
    messages: [{ text: 'Do you still have that Inter alternative you mentioned? The one with the tabular figures', ago: 9 * DAY, from: people.chloe }],
  },
]

function buildMessages(seed: Seed): Message[] {
  return seed.messages.map((item) => {
    const { ago, from, text, ...rest } = item
    return {
      guid: guid(),
      chatGuid: seed.guid,
      text,
      fromMe: !from,
      sender: from,
      date: now - ago,
      service: seed.service,
      attachments: [],
      tapbacks: [],
      isAudio: false,
      ...rest,
    }
  })
}

const replies: Record<string, string[]> = {
  'iMessage;-;+14155550134': ['ha, deal', 'see you at 4 then', 'the wifi there is terrible btw, tether'],
  'iMessage;+;chat240119384759': ['UA 1287, lands 6:40', 'thank you!!', 'I will text when I land'],
  'iMessage;-;priya.natarajan@icloud.com': ['perfect, shipping it', 'you are the best'],
  'SMS;-;+14155550188': ['👍', 'Sounds good'],
}

export class DemoTransport implements Transport {
  readonly kind = 'demo' as const
  private listeners = new Set<(event: TransportEvent) => void>()
  private chats = new Map<string, Chat>()
  private messages = new Map<string, Message[]>()
  private timers: Array<ReturnType<typeof setTimeout>> = []
  private replyIndex = new Map<string, number>()

  constructor() {
    for (const seed of seeds) {
      const list = buildMessages(seed)
      this.messages.set(seed.guid, list)
      const last = list[list.length - 1]
      this.chats.set(seed.guid, {
        guid: seed.guid,
        identifier: seed.identifier,
        service: seed.service,
        isGroup: seed.isGroup,
        displayName: seed.displayName,
        participants: seed.participants,
        pinned: false,
        muted: false,
        archived: false,
        unread: Boolean(last && !last.fromMe && !last.dateRead && seed.guid.endsWith('0199')),
        lastMessage: last,
        lastActivity: last?.date ?? 0,
      })
    }
  }

  private emit(event: TransportEvent): void {
    for (const listener of this.listeners) listener(event)
  }

  private later(ms: number, run: () => void): void {
    this.timers.push(setTimeout(run, ms))
  }

  subscribe(listener: (event: TransportEvent) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  async connect(): Promise<ServerInfo> {
    this.emit({ type: 'connection', status: 'connecting' })
    const info: ServerInfo = { version: 'demo', macosVersion: '15.6', privateApi: true, helperConnected: true, icloudAccount: 'you@icloud.com' }
    this.later(50, () => this.emit({ type: 'connection', status: 'online' }))
    return info
  }

  disconnect(): void {
    for (const timer of this.timers) clearTimeout(timer)
    this.timers = []
  }

  async listChats(): Promise<Page<Chat>> {
    const items = [...this.chats.values()].sort((a, b) => b.lastActivity - a.lastActivity)
    return { items, hasMore: false }
  }

  async getChat(chatGuid: string): Promise<Chat> {
    const chat = this.chats.get(chatGuid)
    if (!chat) throw new Error(`No chat ${chatGuid}`)
    return chat
  }

  async loadMessages(chatGuid: string, options: { limit: number; before?: number }): Promise<Page<Message>> {
    const all = (this.messages.get(chatGuid) ?? []).filter((message) => options.before === undefined || message.date < options.before)
    const items = all.slice(Math.max(0, all.length - options.limit))
    return { items, hasMore: items.length < all.length }
  }

  async searchMessages(query: string, options: { chatGuid?: string; limit?: number; after?: number } = {}): Promise<Message[]> {
    const needle = query.trim().toLowerCase()
    const out: Message[] = []
    for (const [chatGuid, list] of this.messages) {
      if (options.chatGuid && chatGuid !== options.chatGuid) continue
      for (const message of list) {
        if (options.after !== undefined && message.date <= options.after) continue
        if (needle && !message.text.toLowerCase().includes(needle)) continue
        out.push(message)
      }
    }
    out.sort((a, b) => b.date - a.date)
    return out.slice(0, options.limit ?? 50)
  }

  async listContacts(): Promise<Contact[]> {
    return (Object.values(people) as Handle[])
      .filter((handle) => handle.name)
      .map((handle) => ({ id: handle.address, name: handle.name ?? handle.address, addresses: [handle.address] }))
  }

  private push(message: Message, updateChat = true): void {
    const list = this.messages.get(message.chatGuid) ?? []
    const index = list.findIndex((item) => item.guid === message.guid)
    if (index >= 0) list[index] = message
    else list.push(message)
    this.messages.set(message.chatGuid, list)
    const chat = this.chats.get(message.chatGuid)
    if (chat && updateChat && !message.reaction) {
      this.chats.set(message.chatGuid, { ...chat, lastMessage: message, lastActivity: Math.max(chat.lastActivity, message.date) })
    }
    this.emit({ type: 'message', message })
  }

  private scheduleReply(chatGuid: string): void {
    const chat = this.chats.get(chatGuid)
    const pool = replies[chatGuid]
    if (!chat || !pool) return
    const index = this.replyIndex.get(chatGuid) ?? 0
    const text = pool[index % pool.length] ?? 'ok'
    this.replyIndex.set(chatGuid, index + 1)
    const from = chat.participants[0]
    this.later(1400, () => this.emit({ type: 'typing', chatGuid, typing: true }))
    this.later(3600, () => {
      this.emit({ type: 'typing', chatGuid, typing: false })
      this.push({ guid: guid(), chatGuid, text, fromMe: false, sender: from, date: Date.now(), service: chat.service, attachments: [], tapbacks: [], isAudio: false })
    })
  }

  private deliver(message: Message): void {
    this.later(500, () => this.push({ ...message, dateDelivered: Date.now() }, false))
    if (message.chatGuid.endsWith('0134')) this.later(2200, () => this.push({ ...message, dateDelivered: Date.now() - 1700, dateRead: Date.now() }, false))
  }

  async sendText(chatGuid: string, text: string, options: SendTextOptions = {}): Promise<Message> {
    const chat = await this.getChat(chatGuid)
    const message: Message = {
      guid: guid(),
      tempGuid: options.tempGuid,
      chatGuid,
      text,
      subject: options.subject,
      fromMe: true,
      date: Date.now(),
      service: chat.service,
      attachments: [],
      tapbacks: [],
      replyTo: options.replyTo,
      effect: options.effect,
      isAudio: false,
    }
    await new Promise((resolve) => setTimeout(resolve, 180))
    this.push(message)
    this.deliver(message)
    this.scheduleReply(chatGuid)
    return message
  }

  async sendAttachment(chatGuid: string, path: string, options: SendAttachmentOptions = {}): Promise<Message> {
    const chat = await this.getChat(chatGuid)
    const file = Bun.file(path)
    const name = options.name ?? path.split('/').pop() ?? 'attachment'
    const message: Message = {
      guid: guid(),
      tempGuid: options.tempGuid,
      chatGuid,
      text: '',
      fromMe: true,
      date: Date.now(),
      service: chat.service,
      attachments: [{ guid: `demo-upload-${seq}`, name, mime: file.type || 'application/octet-stream', bytes: file.size, isSticker: false, localPath: path }],
      tapbacks: [],
      isAudio: Boolean(options.isAudio),
    }
    await new Promise((resolve) => setTimeout(resolve, 400))
    this.push(message)
    this.deliver(message)
    return message
  }

  async attachmentPath(attachmentGuid: string): Promise<string> {
    const src = photos[attachmentGuid]
    if (!src) throw new Error(`Attachment ${attachmentGuid} is not in the demo set`)
    return src
  }

  async createChat(addresses: string[], firstMessage: string, service: Service = 'iMessage'): Promise<Chat> {
    const first = addresses[0] ?? 'unknown'
    const chatGuid = addresses.length > 1 ? `${service};+;chat${Date.now()}` : `${service};-;${first}`
    const participants: Handle[] = addresses.map((address) => Object.values(people).find((p) => p.address === address) ?? { address, service })
    const chat: Chat = {
      guid: chatGuid,
      identifier: addresses.length > 1 ? chatGuid : first,
      service,
      isGroup: addresses.length > 1,
      participants,
      pinned: false,
      muted: false,
      archived: false,
      unread: false,
      lastActivity: Date.now(),
    }
    this.chats.set(chatGuid, chat)
    this.messages.set(chatGuid, [])
    this.emit({ type: 'chat', chat })
    if (firstMessage) await this.sendText(chatGuid, firstMessage)
    return this.chats.get(chatGuid) ?? chat
  }

  async markRead(chatGuid: string): Promise<void> {
    this.emit({ type: 'read', chatGuid, read: true })
  }

  async markUnread(chatGuid: string): Promise<void> {
    this.emit({ type: 'read', chatGuid, read: false })
  }

  async deleteChat(chatGuid: string): Promise<void> {
    this.chats.delete(chatGuid)
    this.messages.delete(chatGuid)
    this.emit({ type: 'chat-removed', chatGuid })
  }

  async react(chatGuid: string, messageGuid: string, kind: TapbackKind, options: { emoji?: string; remove?: boolean } = {}): Promise<void> {
    const chat = await this.getChat(chatGuid)
    const reaction: Message = {
      guid: guid(),
      chatGuid,
      text: '',
      fromMe: true,
      date: Date.now(),
      service: chat.service,
      attachments: [],
      tapbacks: [],
      isAudio: false,
      reaction: { targetGuid: messageGuid, kind, emoji: options.emoji, removed: Boolean(options.remove) },
    }
    this.later(250, () => this.push(reaction, false))
  }

  async setTyping(): Promise<void> {}

  async editMessage(chatGuid: string, messageGuid: string, text: string): Promise<Message> {
    const list = this.messages.get(chatGuid) ?? []
    const target = list.find((item) => item.guid === messageGuid)
    if (!target) throw new Error('Message not found')
    const updated = { ...target, text, dateEdited: Date.now() }
    this.push(updated, false)
    return updated
  }

  async unsendMessage(chatGuid: string, messageGuid: string): Promise<void> {
    const list = this.messages.get(chatGuid) ?? []
    const target = list.find((item) => item.guid === messageGuid)
    if (!target) throw new Error('Message not found')
    this.push({ ...target, dateRetracted: Date.now() }, false)
  }

  async renameGroup(chatGuid: string, name: string): Promise<void> {
    const chat = await this.getChat(chatGuid)
    const next = { ...chat, displayName: name }
    this.chats.set(chatGuid, next)
    this.push({ guid: guid(), chatGuid, text: '', fromMe: true, date: Date.now(), service: chat.service, attachments: [], tapbacks: [], isAudio: false, groupEvent: { kind: 'rename', title: name } })
    this.emit({ type: 'chat', chat: next })
  }

  async addParticipant(chatGuid: string, address: string): Promise<void> {
    const chat = await this.getChat(chatGuid)
    const who: Handle = { address, service: chat.service }
    const next = { ...chat, participants: [...chat.participants, who] }
    this.chats.set(chatGuid, next)
    this.push({ guid: guid(), chatGuid, text: '', fromMe: true, date: Date.now(), service: chat.service, attachments: [], tapbacks: [], isAudio: false, groupEvent: { kind: 'join', who } })
    this.emit({ type: 'chat', chat: next })
  }

  async removeParticipant(chatGuid: string, address: string): Promise<void> {
    const chat = await this.getChat(chatGuid)
    const who = chat.participants.find((item) => item.address === address)
    const next = { ...chat, participants: chat.participants.filter((item) => item.address !== address) }
    this.chats.set(chatGuid, next)
    this.push({ guid: guid(), chatGuid, text: '', fromMe: true, date: Date.now(), service: chat.service, attachments: [], tapbacks: [], isAudio: false, groupEvent: { kind: 'leave', who } })
    this.emit({ type: 'chat', chat: next })
  }

  async leaveGroup(chatGuid: string): Promise<void> {
    const chat = await this.getChat(chatGuid)
    this.push({ guid: guid(), chatGuid, text: '', fromMe: true, date: Date.now(), service: chat.service, attachments: [], tapbacks: [], isAudio: false, groupEvent: { kind: 'leave' } })
  }

  async setGroupIcon(): Promise<void> {}

  async notifySilenced(): Promise<void> {}

  async startFaceTime(): Promise<string | undefined> {
    return 'https://facetime.apple.com/join#v=1&p=demo'
  }
}
