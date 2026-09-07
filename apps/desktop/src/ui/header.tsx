import { Fragment, memo, useEffect, useMemo, useRef, useState } from 'react'
import {
  chatTitle,
  conversationFocus,
  conversationHandles,
  conversationHasOlder,
  conversationLoading,
  conversationMessages,
  copyText,
  formatAddress,
  formatBytes,
  handleName,
  matchFriend,
  openExternal,
  type Attachment,
  type Chat,
  type Handle,
  type Message,
  type SummarizeMessage,
  contactAddresses,
} from '@messages/core'
import { C, INFO_WIDTH, RADIUS, S, TITLEBAR_HEIGHT, TYPE } from './theme'
import { Icon, type IconName } from './icons'
import { LocationCard } from './location'
import { Avatar, Button, Divider, IconButton, SectionLabel, TextField, overlayShadow } from './primitives'
import { shortcut, useShell, type MenuItem } from './context'
import { chatMenu, confirmDelete } from './sidebar'
import { AUTO_DOWNLOAD_BYTES, generateTile } from './bubble'
import { slots } from './limit'
import { useAppState } from './use-app-state'
import { DURATION, Fade } from './motion'

/** Caps how far back "Catch me up" looks when I have not sent anything in this thread. */
const CATCH_UP_FALLBACK = 50

/** Everything since my last message in the thread, oldest first, or the last 50 if I never sent one. */
function messagesToCatchUpOn(messages: Message[]): Message[] {
  const visible = messages.filter((message) => !message.dateRetracted)
  const lastMineIndex = visible.reduce((found, message, index) => (message.fromMe ? index : found), -1)
  const since = lastMineIndex >= 0 ? visible.slice(lastMineIndex + 1) : []
  return since.length > 0 ? since : visible.slice(-CATCH_UP_FALLBACK)
}

/** Only text, sender name and time leave the machine; attachment bytes never do. */
function toSummarizeMessages(messages: Message[]): SummarizeMessage[] {
  return messages.map((message) => ({
    sender: message.fromMe ? 'Me' : message.sender ? handleName(message.sender) : 'Someone',
    text: message.text.trim() || (message.attachments.length > 0 ? '[attachment]' : ''),
    date: message.date,
    fromMe: message.fromMe,
  }))
}

interface CatchUpCard {
  x: number
  loading: boolean
  error?: string
  summary?: string
}

function CatchMeUp({ chat, messages }: { chat: Chat; messages: Message[] }) {
  const shell = useShell()
  const [card, setCard] = useState<CatchUpCard | null>(null)
  const controllerRef = useRef<AbortController | null>(null)

  // Switching threads makes any answer in flight stale; drop it rather than show it on the wrong chat.
  useEffect(() => {
    controllerRef.current?.abort()
    controllerRef.current = null
    setCard(null)
  }, [chat.guid])

  const assistant = shell.assistant
  if (!assistant) return null

  const close = () => {
    controllerRef.current?.abort()
    controllerRef.current = null
    setCard(null)
  }

  const run = (x: number) => {
    const controller = new AbortController()
    controllerRef.current = controller
    setCard({ x, loading: true })
    assistant
      .summarize(toSummarizeMessages(messagesToCatchUpOn(messages)), { signal: controller.signal })
      .then((summary) => {
        if (controllerRef.current !== controller) return
        setCard({ x, loading: false, summary })
      })
      .catch((error: unknown) => {
        if (controllerRef.current !== controller) return
        setCard({ x, loading: false, error: error instanceof Error ? error.message : String(error) })
      })
  }

  return (
    <>
      <IconButton icon="sparkles" label="Catch me up" testId="catch-me-up" size={17} disabled={card?.loading ?? false} onClick={(event) => (card ? close() : run(event.x ?? 0))} />
      {card ? (
        <anchored deferred occlude priority={3} position={{ x: card.x, y: TITLEBAR_HEIGHT }} anchor="topRight" fit="snap" snapMargin={S.x2}>
          <Fade enter={DURATION.fast}>
            <div
              testId="catch-up-card"
              style={{
                display: 'flex',
                flexDirection: 'column',
                width: 300,
                padding: S.x3,
                gap: S.x2,
                borderRadius: RADIUS.card,
                backgroundColor: C.overlay,
                borderWidth: 1,
                borderColor: C.overlayBorder,
                boxShadow: overlayShadow,
              }}
            >
              <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x2 }}>
                <Icon name="sparkles" size={14} color={C.accent} />
                <text style={{ ...TYPE.body, fontWeight: 600, color: C.text, flexGrow: 1 }}>Catch me up</text>
                <IconButton icon="close" label="Close" size={12} hit={22} onClick={close} />
              </div>
              {card.loading ? (
                <text style={{ ...TYPE.caption, color: C.secondary }}>Summarizing…</text>
              ) : card.error ? (
                <text testId="catch-up-error" style={{ ...TYPE.caption, color: C.danger }}>
                  {card.error}
                </text>
              ) : (
                <text testId="catch-up-summary" style={{ ...TYPE.body, color: C.text }}>
                  {card.summary}
                </text>
              )}
            </div>
          </Fade>
        </anchored>
      ) : null}
    </>
  )
}

export function ConversationHeader({ chat, infoOpen }: { chat: Chat; infoOpen: boolean }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const title = chatTitle(chat)
  const handles = conversationHandles(state, chat.guid)
  const first = handles[0]
  const sharing = !chat.isGroup && first ? matchFriend(Object.values(state.locations), [first.address, ...contactAddresses(state.contacts, first.address)]) : undefined
  const silenced = conversationFocus(state, chat.guid) === 'silenced'
  const subtitle =
    (chat.isGroup
      ? chat.participants.map(handleName).join(', ')
      : first && first.name
        ? `${handles.map((handle) => formatAddress(handle.address)).join(' · ')} · ${chat.service}`
        : chat.service === 'iMessage'
          ? 'iMessage'
          : 'Text message') + (sharing ? ' · Sharing location' : '')

  const faceTime = () => void shell.store.startFaceTime(chat.guid)

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: S.x2,
        height: TITLEBAR_HEIGHT,
        paddingLeft: S.x3,
        paddingRight: S.x2,
        flexShrink: 0,
        borderBottomWidth: 1,
        borderColor: C.separator,
        userSelect: 'none',
      }}
    >
      <div
        testId="thread-identity"
        tabIndex={0}
        onClick={shell.toggleInfo}
        onKeyDown={(event) => {
          if (event.key === 'enter' || event.key === 'space') shell.toggleInfo()
        }}
        onAuxClick={(event) => {
          if (event.isRightClick) shell.openMenu({ x: event.x ?? 0, y: event.y ?? 0, items: chatMenu(chat, shell) })
        }}
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          gap: S.x2,
          flexGrow: 1,
          flexShrink: 1,
          flexBasis: 0,
          minWidth: 0,
          height: 40,
          paddingLeft: S.x1,
          paddingRight: S.x2,
          borderRadius: RADIUS.row,
          cursor: 'pointer',
          hover: { backgroundColor: C.hoverWash },
          active: { backgroundColor: C.pressWash },
        }}
      >
        <Avatar chat={chat} size={30} />
        <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0 }}>
          <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x1, minWidth: 0 }}>
            <text testId="thread-title" style={{ ...TYPE.title, color: C.text, flexShrink: 1, minWidth: 0, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>
              {title}
            </text>
            {silenced ? <Icon name="silenced" size={12} color={C.tertiary} /> : null}
          </div>
          <text style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{subtitle}</text>
        </div>
      </div>
      {state.capabilities.facetime && !chat.isGroup ? <IconButton icon="video" label="FaceTime" size={17} onClick={faceTime} /> : null}
      <CatchMeUp chat={chat} messages={state.messages[chat.guid] ?? []} />
      <IconButton
        icon="info"
        label={`${infoOpen ? 'Hide details' : 'Show details'} (${shortcut('I')})`}
        testId="info"
        size={17}
        active={infoOpen}
        onClick={shell.toggleInfo}
      />
    </div>
  )
}

function Row({
  icon,
  label,
  value,
  onClick,
  danger,
  testId,
}: {
  icon: IconName
  label: string
  value?: string
  onClick?: () => void
  danger?: boolean
  testId?: string
}) {
  const color = danger ? C.danger : C.text
  return (
    <div
      testId={testId}
      tabIndex={onClick ? 0 : undefined}
      onClick={onClick}
      onKeyDown={(event) => {
        if (event.key === 'enter' || event.key === 'space') onClick?.()
      }}
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: S.x2,
        height: 32,
        paddingLeft: S.x2,
        paddingRight: S.x2,
        borderRadius: RADIUS.control,
        flexShrink: 0,
        cursor: onClick ? 'pointer' : 'default',
        hover: onClick ? { backgroundColor: danger ? C.dangerSoft : C.raised } : undefined,
        active: onClick ? { backgroundColor: danger ? C.dangerSoft : C.raisedHover } : undefined,
      }}
    >
      <div style={{ width: 15, display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
        <Icon name={icon} size={15} color={danger ? C.danger : C.secondary} />
      </div>
      <text style={{ ...TYPE.body, color, flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{label}</text>
      {value ? <text style={{ ...TYPE.body, color: C.secondary, whiteSpace: 'nowrap', flexShrink: 0 }}>{value}</text> : null}
    </div>
  )
}

function participantMenu(handle: Handle, chat: Chat, shell: ReturnType<typeof useShell>, manage: boolean): MenuItem[] {
  const items: MenuItem[] = [{ label: 'Copy address', icon: 'copy', onSelect: () => void copyText(handle.address) }]
  if (manage) {
    items.push(
      { kind: 'separator' },
      {
        label: `Remove ${handleName(handle)}`,
        icon: 'removePerson',
        danger: true,
        disabled: chat.participants.length <= 2,
        onSelect: () => void shell.store.removeParticipant(chat.guid, handle.address),
      },
    )
  }
  return items
}

/** One person. A merged conversation lists every address the person is reached on under the one name. */
function Participant({ handle, chat, manage, addresses = [handle.address] }: { handle: Handle; chat: Chat; manage: boolean; addresses?: string[] }) {
  const shell = useShell()
  return (
    <div
      testId={`participant-${handle.address}`}
      tabIndex={0}
      onAuxClick={(event) => {
        if (event.isRightClick) shell.openMenu({ x: event.x ?? 0, y: event.y ?? 0, items: participantMenu(handle, chat, shell, manage) })
      }}
      onKeyDown={(event) => {
        if (event.key === 'enter' || event.key === 'space') void copyText(handle.address)
      }}
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: S.x2,
        minHeight: 40,
        paddingTop: S.x1,
        paddingBottom: S.x1,
        paddingLeft: S.x2,
        paddingRight: S.x1,
        borderRadius: RADIUS.control,
        flexShrink: 0,
        hover: { backgroundColor: C.raised },
      }}
    >
      <Avatar handle={handle} size={28} />
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0 }}>
        <text style={{ ...TYPE.body, color: C.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{handleName(handle)}</text>
        {handle.name
          ? addresses.map((address) => (
              <text key={address} style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>
                {formatAddress(address)}
              </text>
            ))
          : null}
      </div>
      {manage && chat.participants.length > 2 ? (
        <IconButton icon="close" label={`Remove ${handleName(handle)}`} size={12} hit={24} onClick={() => void shell.store.removeParticipant(chat.guid, handle.address)} />
      ) : null}
    </div>
  )
}

/** A group lists its participants; a one-to-one conversation is one person with every address it is reached on. */
function people(chat: Chat, state: ReturnType<typeof useAppState>): Array<{ handle: Handle; addresses: string[] }> {
  if (chat.isGroup) return chat.participants.map((handle) => ({ handle, addresses: [handle.address] }))
  const handles = conversationHandles(state, chat.guid)
  const first = handles[0]
  return first ? [{ handle: first, addresses: handles.map((handle) => handle.address) }] : []
}

interface GalleryItem {
  attachment: Attachment
  message: Message
}

const GALLERY_COLUMNS = 3
const GALLERY_GAP = S.x1
/** INFO_WIDTH minus the panel's own S.x4 inset on each side minus the two gaps between columns, split three ways. */
const GALLERY_THUMB = (INFO_WIDTH - S.x4 * 2 - GALLERY_GAP * (GALLERY_COLUMNS - 1)) / GALLERY_COLUMNS
/** Twice the box, so the cut still has pixels to spare on a HiDPI screen. */
const GALLERY_TILE = GALLERY_THUMB * 2
/**
 * Photos the panel fetches at once. It mounts every picture in the
 * conversation the moment it opens, so without this a chat with a hundred of
 * them started a hundred requests, and each one that landed republished the
 * store and re-rendered the window.
 */
const gallerySlot = slots(3)

/** Newest first: every visible, non-sticker attachment across what is loaded, images for the grid and everything else for the file list. */
function galleryItems(messages: Message[]): { images: GalleryItem[]; files: GalleryItem[] } {
  const images: GalleryItem[] = []
  const files: GalleryItem[] = []
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index]!
    for (const attachment of message.attachments) {
      if (attachment.hidden || attachment.isSticker) continue
      ;(attachment.mime.startsWith('image/') ? images : files).push({ attachment, message })
    }
  }
  return { images, files }
}

const GalleryThumbnail = memo(function GalleryThumbnail({ attachment, message }: { attachment: Attachment; message: Message }) {
  const shell = useShell()
  const [failed, setFailed] = useState(false)
  const [thumb, setThumb] = useState<string | null>(null)
  const src = attachment.localPath
  // Anything past the cap waits for a click, the same bargain the thread makes.
  const wanted = !src && !failed && attachment.bytes <= AUTO_DOWNLOAD_BYTES
  useEffect(() => {
    if (!wanted) return
    let cancelled = false
    void gallerySlot(() => shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime)).catch(() => {
      if (!cancelled) setFailed(true)
    })
    return () => {
      cancelled = true
    }
  }, [wanted, shell.store, message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime])
  // An eighty pixel box was being handed the whole photo, so a panel of them
  // asked the renderer for gigabytes of texture. Nothing is painted until the
  // square cut is settled.
  useEffect(() => {
    if (!src) return
    let cancelled = false
    setThumb(null)
    void generateTile(src, attachment.guid, 1, GALLERY_TILE).then((path) => {
      if (!cancelled) setThumb(path ?? src)
    })
    return () => {
      cancelled = true
    }
  }, [src, attachment.guid])
  return (
    <div
      testId={`gallery-photo-${attachment.guid}`}
      onClick={() => shell.openLightbox({ chatGuid: message.chatGuid, attachmentGuid: attachment.guid })}
      style={{ width: GALLERY_THUMB, height: GALLERY_THUMB, borderRadius: RADIUS.control, overflow: 'hidden', backgroundColor: C.raised, cursor: 'pointer', hover: { opacity: 0.9 } }}
    >
      {thumb ? <img src={thumb} objectFit="contain" style={{ width: GALLERY_THUMB, height: GALLERY_THUMB }} /> : null}
    </div>
  )
})

const GalleryFile = memo(function GalleryFile({ attachment, message }: { attachment: Attachment; message: Message }) {
  const shell = useShell()
  const open = async () => {
    const local = attachment.localPath ?? (await shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => undefined))
    if (local) openExternal(local)
  }
  return (
    <div
      testId={`gallery-file-${attachment.guid}`}
      tabIndex={0}
      onClick={() => void open()}
      onKeyDown={(event) => {
        if (event.key === 'enter' || event.key === 'space') void open()
      }}
      style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x2, height: 36, paddingLeft: S.x2, paddingRight: S.x2, borderRadius: RADIUS.control, cursor: 'pointer', flexShrink: 0, hover: { backgroundColor: C.raised } }}
    >
      <Icon name={message.isAudio ? 'audio' : 'file'} size={15} color={C.secondary} />
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0 }}>
        <text style={{ ...TYPE.body, color: C.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{message.isAudio ? 'Audio message' : attachment.name}</text>
        <text style={{ ...TYPE.micro, color: C.secondary }}>{formatBytes(attachment.bytes)}</text>
      </div>
    </div>
  )
})

function GallerySection({ chat }: { chat: Chat }) {
  const shell = useShell()
  const { store } = shell
  const state = useAppState(store)
  const messages = useMemo(() => conversationMessages(state, chat.guid), [state.messages, state.merged, state.primaryOf, chat.guid])
  const { images, files } = useMemo(() => galleryItems(messages), [messages])
  const hasOlder = conversationHasOlder(state, chat.guid)
  const loading = conversationLoading(state, chat.guid)

  if (images.length === 0 && files.length === 0 && !hasOlder) return null

  return (
    <>
      <div style={{ paddingTop: S.x4, paddingBottom: S.x4, paddingLeft: S.x4, paddingRight: S.x4, flexShrink: 0 }}>
        <Divider />
      </div>
      <SectionLabel inset={S.x4}>Photos and files</SectionLabel>
      {images.length > 0 ? (
        <div style={{ display: 'flex', flexDirection: 'row', flexWrap: 'wrap', gap: GALLERY_GAP, paddingLeft: S.x4, paddingRight: S.x4, flexShrink: 0 }}>
          {images.map(({ attachment, message }) => (
            <GalleryThumbnail key={attachment.guid} attachment={attachment} message={message} />
          ))}
        </div>
      ) : null}
      {files.length > 0 ? (
        <div style={{ display: 'flex', flexDirection: 'column', paddingLeft: S.x2, paddingRight: S.x2, paddingTop: images.length > 0 ? S.x2 : 0, flexShrink: 0 }}>
          {files.map(({ attachment, message }) => (
            <GalleryFile key={attachment.guid} attachment={attachment} message={message} />
          ))}
        </div>
      ) : null}
      {hasOlder ? (
        <div style={{ paddingLeft: S.x4, paddingRight: S.x4, paddingTop: S.x2, paddingBottom: S.x2, flexShrink: 0 }}>
          <Button testId="gallery-load-older" onClick={() => void store.loadEarlier(chat.guid)} disabled={loading}>
            {loading ? 'Loading…' : 'Load older'}
          </Button>
        </div>
      ) : null}
    </>
  )
}

/** Sized to `INFO_WIDTH` whatever holds it: the app clips or slides the box around it. */
export function InfoPanel({ chat }: { chat: Chat }) {
  const shell = useShell()
  const { store } = shell
  const state = useAppState(store)
  const [name, setName] = useState(chat.displayName ?? '')
  const [address, setAddress] = useState('')
  const manage = state.capabilities.groupManagement && chat.isGroup
  const addPerson = () => {
    const target = address.trim()
    if (!target) return
    setAddress('')
    void store.addParticipant(chat.guid, target)
  }
  const leave = () =>
    shell.confirm({
      title: `Leave “${chatTitle(chat)}”?`,
      body: 'You stop getting its messages. Someone in the group can add you back.',
      action: 'Leave',
      danger: true,
      onConfirm: () => void store.leaveGroup(chat.guid),
    })

  // The details panel is the only Find My consumer, so it drives the store's poll.
  useEffect(() => {
    store.setDetailsOpen(true)
    return () => store.setDetailsOpen(false)
  }, [store])

  return (
    <div
      testId="info-panel"
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: INFO_WIDTH,
        flexShrink: 0,
        height: '100%',
        backgroundColor: C.sidebar,
        borderLeftWidth: 1,
        borderColor: C.sidebarBorder,
        userSelect: 'none',
      }}
    >
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', height: TITLEBAR_HEIGHT, paddingLeft: S.x4, paddingRight: S.x2, flexShrink: 0 }}>
        <text style={{ ...TYPE.title, color: C.text, flexGrow: 1 }}>Details</text>
        <IconButton icon="close" label={`Close details (${shortcut('I')})`} testId="close-info" onClick={shell.toggleInfo} />
      </div>

      {/* A div with overflow takes scrollTo but never the wheel, so the panel
          sat still and the wheel went to whatever was behind it. The sidebar
          already scrolls through a list. `pointerEvents: auto` is what stops
          the wheel here rather than letting it through to the thread; unset,
          gpuix passes it on the way HTML does. */}
      <virtual-list estimatedItemHeight={72} overdraw={600} style={{ flexGrow: 1, minHeight: 0, width: '100%', pointerEvents: 'auto' }}>
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: S.x2, paddingTop: S.x1, paddingBottom: S.x5, paddingLeft: S.x4, paddingRight: S.x4, flexShrink: 0 }}>
          <Avatar chat={chat} size={72} />
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2 }}>
            <text style={{ ...TYPE.large, fontSize: 17, lineHeight: 22, color: C.text, textAlign: 'center' }}>{chatTitle(chat)}</text>
            <text style={{ ...TYPE.caption, color: C.secondary, textAlign: 'center' }}>
              {chat.isGroup ? `${chat.participants.length} people · ${chat.service}` : chat.service}
            </text>
          </div>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', paddingLeft: S.x2, paddingRight: S.x2, gap: 2, flexShrink: 0 }}>
          <Row icon={chat.pinned ? 'pinOff' : 'pin'} label={chat.pinned ? 'Unpin' : 'Pin'} testId="toggle-pin" onClick={() => store.togglePin(chat.guid)} />
          <Row icon={chat.muted ? 'unmute' : 'mute'} label={chat.muted ? 'Show alerts' : 'Hide alerts'} testId="toggle-mute" onClick={() => store.toggleMute(chat.guid)} />
          <Row
            icon={chat.readReceipts === false ? 'eye' : 'eyeOff'}
            label={chat.readReceipts === false ? 'Send read receipts' : 'Read without receipts'}
            testId="toggle-read-receipts"
            onClick={() => store.toggleReadReceipts(chat.guid)}
          />
          <Row icon="markUnread" label="Mark as unread" value={shortcut('U', { shift: true })} onClick={() => void store.markUnread(chat.guid)} />
        </div>

        <div style={{ paddingTop: S.x4, paddingBottom: S.x4, paddingLeft: S.x4, paddingRight: S.x4, flexShrink: 0 }}>
          <Divider />
        </div>

        <SectionLabel inset={S.x4}>{chat.isGroup ? 'People' : 'Contact'}</SectionLabel>
        <div style={{ display: 'flex', flexDirection: 'column', paddingLeft: S.x2, paddingRight: S.x2, flexShrink: 0 }}>
          {people(chat, state).map(({ handle, addresses }) => {
            const location = matchFriend(Object.values(state.locations), [...addresses, ...contactAddresses(state.contacts, handle.address)])
            return (
              <Fragment key={handle.address}>
                <Participant handle={handle} chat={chat} manage={manage} addresses={addresses} />
                {location ? (
                  <>
                    <SectionLabel inset={S.x2}>Location</SectionLabel>
                    <LocationCard handle={handle} location={location} />
                  </>
                ) : null}
              </Fragment>
            )
          })}
          {state.findMy === 'unavailable' ? (
            <text style={{ ...TYPE.caption, color: C.secondary, paddingLeft: S.x2, paddingTop: S.x1 }}>Find My needs the Mac agent (see README)</text>
          ) : null}
        </div>

        <GallerySection chat={chat} />

        {manage ? (
          <div style={{ display: 'flex', flexDirection: 'column', gap: S.x2, paddingTop: S.x3, paddingLeft: S.x4, paddingRight: S.x4, flexShrink: 0 }}>
            <div style={{ display: 'flex', flexDirection: 'row', gap: S.x2 }}>
              <TextField value={address} onChange={setAddress} placeholder="Phone number or email" onSubmit={addPerson} />
              <Button onClick={addPerson} disabled={address.trim().length === 0}>
                Add
              </Button>
            </div>
            <div style={{ display: 'flex', flexDirection: 'row', gap: S.x2 }}>
              <TextField value={name} onChange={setName} placeholder="Group name" onSubmit={() => void store.renameGroup(chat.guid, name.trim())} />
              <Button onClick={() => void store.renameGroup(chat.guid, name.trim())} disabled={name.trim() === (chat.displayName ?? '')}>
                Rename
              </Button>
            </div>
          </div>
        ) : null}

        <div style={{ paddingTop: S.x4, paddingBottom: S.x4, paddingLeft: S.x4, paddingRight: S.x4, flexShrink: 0 }}>
          <Divider />
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', paddingLeft: S.x2, paddingRight: S.x2, gap: 2, paddingBottom: S.x4, flexShrink: 0 }}>
          <Row
            icon="download"
            label={state.exportingChat === chat.guid ? 'Exporting…' : 'Export conversation…'}
            testId="export-chat"
            onClick={() => void store.exportConversation(chat.guid)}
          />
          {chat.isGroup && state.capabilities.groupManagement ? <Row icon="leave" label="Leave conversation" danger onClick={leave} /> : null}
          <Row icon="trash" label="Delete conversation" danger testId="delete-chat" onClick={() => confirmDelete(chat, shell)} />
        </div>
      </virtual-list>
    </div>
  )
}
