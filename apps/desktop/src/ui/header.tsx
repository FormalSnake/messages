import { Fragment, useEffect, useMemo, useState } from 'react'
import {
  chatTitle,
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
  contactAddresses,
} from '@messages/core'
import { C, INFO_WIDTH, RADIUS, S, TITLEBAR_HEIGHT, TYPE } from './theme'
import { Icon, type IconName } from './icons'
import { LocationCard } from './location'
import { Avatar, Button, Divider, IconButton, SectionLabel, TextField } from './primitives'
import { shortcut, useShell, type MenuItem } from './context'
import { chatMenu, confirmDelete } from './sidebar'
import { useAppState } from './use-app-state'

export function ConversationHeader({ chat, infoOpen }: { chat: Chat; infoOpen: boolean }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const title = chatTitle(chat)
  const handles = conversationHandles(state, chat.guid)
  const first = handles[0]
  const sharing = !chat.isGroup && first ? matchFriend(Object.values(state.locations), [first.address, ...contactAddresses(state.contacts, first.address)]) : undefined
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
          <text testId="thread-title" style={{ ...TYPE.title, color: C.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>
            {title}
          </text>
          <text style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{subtitle}</text>
        </div>
      </div>
      {state.capabilities.facetime && !chat.isGroup ? <IconButton icon="video" label="FaceTime" size={17} onClick={faceTime} /> : null}
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
        onSelect: () => void shell.store.transport.removeParticipant(chat.guid, handle.address),
      },
    )
  }
  return items
}

function Participant({ handle, chat, manage }: { handle: Handle; chat: Chat; manage: boolean }) {
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
        height: 40,
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
        {handle.name ? <text style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{formatAddress(handle.address)}</text> : null}
      </div>
      {manage && chat.participants.length > 2 ? (
        <IconButton icon="close" label={`Remove ${handleName(handle)}`} size={12} hit={24} onClick={() => void shell.store.transport.removeParticipant(chat.guid, handle.address)} />
      ) : null}
    </div>
  )
}

interface GalleryItem {
  attachment: Attachment
  message: Message
}

const GALLERY_COLUMNS = 3
const GALLERY_GAP = S.x1
/** INFO_WIDTH minus the panel's own S.x4 inset on each side minus the two gaps between columns, split three ways. */
const GALLERY_THUMB = (INFO_WIDTH - S.x4 * 2 - GALLERY_GAP * (GALLERY_COLUMNS - 1)) / GALLERY_COLUMNS

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

function GalleryThumbnail({ attachment, message }: { attachment: Attachment; message: Message }) {
  const shell = useShell()
  const [failed, setFailed] = useState(false)
  const src = attachment.localPath
  useEffect(() => {
    if (src || failed) return
    shell.store.attachmentSrc(message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime).catch(() => setFailed(true))
  }, [src, failed, shell.store, message.chatGuid, message.guid, attachment.guid, attachment.name, attachment.mime])
  return (
    <div
      testId={`gallery-photo-${attachment.guid}`}
      onClick={() => shell.openLightbox({ chatGuid: message.chatGuid, attachmentGuid: attachment.guid })}
      style={{ width: GALLERY_THUMB, height: GALLERY_THUMB, borderRadius: RADIUS.control, overflow: 'hidden', backgroundColor: C.raised, cursor: 'pointer', hover: { opacity: 0.9 } }}
    >
      {src ? <img src={src} objectFit="cover" style={{ width: GALLERY_THUMB, height: GALLERY_THUMB }} /> : null}
    </div>
  )
}

function GalleryFile({ attachment, message }: { attachment: Attachment; message: Message }) {
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
}

function GallerySection({ chat }: { chat: Chat }) {
  const shell = useShell()
  const { store } = shell
  const state = useAppState(store)
  const messages = conversationMessages(state, chat.guid)
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

export function InfoPanel({ chat, floating }: { chat: Chat; floating: boolean }) {
  const shell = useShell()
  const { store } = shell
  const state = useAppState(store)
  const [name, setName] = useState(chat.displayName ?? '')
  const [address, setAddress] = useState('')
  const manage = state.capabilities.groupManagement && chat.isGroup
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
        overflowY: 'scroll',
        userSelect: 'none',
        ...(floating
          ? { position: 'absolute', top: 0, right: 0, bottom: 0, boxShadow: { offsetX: -8, offsetY: 0, blurRadius: 32, spreadRadius: 0, color: '#000000a6' } }
          : {}),
      }}
    >
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', height: TITLEBAR_HEIGHT, paddingLeft: S.x4, paddingRight: S.x2, flexShrink: 0 }}>
        <text style={{ ...TYPE.title, color: C.text, flexGrow: 1 }}>Details</text>
        <IconButton icon="close" label={`Close details (${shortcut('I')})`} testId="close-info" onClick={shell.toggleInfo} />
      </div>

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
        <Row icon="markUnread" label="Mark as unread" value={shortcut('U', { shift: true })} onClick={() => void store.markUnread(chat.guid)} />
      </div>

      <div style={{ paddingTop: S.x4, paddingBottom: S.x4, paddingLeft: S.x4, paddingRight: S.x4, flexShrink: 0 }}>
        <Divider />
      </div>

      <SectionLabel inset={S.x4}>{chat.isGroup ? 'People' : 'Contact'}</SectionLabel>
      <div style={{ display: 'flex', flexDirection: 'column', paddingLeft: S.x2, paddingRight: S.x2, flexShrink: 0 }}>
        {(chat.isGroup ? chat.participants : conversationHandles(state, chat.guid)).map((handle) => {
          const location = matchFriend(Object.values(state.locations), [handle.address, ...contactAddresses(state.contacts, handle.address)])
          return (
            <Fragment key={handle.address}>
              <Participant handle={handle} chat={chat} manage={manage} />
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
            <TextField
              value={address}
              onChange={setAddress}
              placeholder="Phone number or email"
              onSubmit={() => void store.transport.addParticipant(chat.guid, address.trim()).then(() => setAddress(''))}
            />
            <Button onClick={() => void store.transport.addParticipant(chat.guid, address.trim()).then(() => setAddress(''))} disabled={address.trim().length === 0}>
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
        {chat.isGroup && state.capabilities.groupManagement ? <Row icon="leave" label="Leave conversation" danger onClick={leave} /> : null}
        <Row icon="trash" label="Delete conversation" danger testId="delete-chat" onClick={() => confirmDelete(chat, shell)} />
      </div>
    </div>
  )
}
