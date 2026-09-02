import { useState } from 'react'
import { chatTitle, formatAddress, handleName, openExternal, type Chat } from '@messages/core'
import { C, RADIUS, TITLEBAR_HEIGHT, TYPE } from './theme'
import { Icon, type IconName } from './icons'
import { Avatar, Button, Divider, IconButton, TextField } from './primitives'
import { useShell } from './context'
import { useAppState } from './use-app-state'

export function ConversationHeader({ chat, infoOpen }: { chat: Chat; infoOpen: boolean }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const title = chatTitle(chat)
  const first = chat.participants[0]
  const subtitle = chat.isGroup
    ? chat.participants.map(handleName).join(', ')
    : first && first.name
      ? `${formatAddress(first.address)} · ${chat.service}`
      : chat.service === 'iMessage'
        ? 'iMessage'
        : 'Text message'

  const faceTime = async () => {
    if (!first) return
    const link = await shell.store.transport.startFaceTime(first.address).catch(() => undefined)
    if (link) openExternal(link)
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10, height: TITLEBAR_HEIGHT, paddingLeft: 16, paddingRight: 8, flexShrink: 0, borderBottomWidth: 1, borderColor: C.sidebarBorder, userSelect: 'none' }}>
      <Avatar chat={chat} size={32} />
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minWidth: 0 }}>
        <text testId="thread-title" style={{ ...TYPE.title, color: C.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{title}</text>
        <text style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{subtitle}</text>
      </div>
      {state.capabilities.facetime && !chat.isGroup ? <IconButton icon="video" label="FaceTime" size={18} onClick={() => void faceTime()} /> : null}
      <IconButton icon="info" label={infoOpen ? 'Hide details' : 'Show details'} testId="info" size={18} active={infoOpen} onClick={shell.toggleInfo} />
    </div>
  )
}

function Row({ icon, label, value, onClick, danger, testId }: { icon: IconName; label: string; value?: string; onClick?: () => void; danger?: boolean; testId?: string }) {
  return (
    <div
      testId={testId}
      onClick={onClick}
      style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10, height: 36, paddingLeft: 10, paddingRight: 10, borderRadius: RADIUS.control, cursor: onClick ? 'pointer' : 'default', hover: onClick ? { backgroundColor: C.raised } : undefined }}
    >
      <Icon name={icon} size={15} color={danger ? C.danger : C.secondary} />
      <text style={{ ...TYPE.body, color: danger ? C.danger : C.text, flexGrow: 1 }}>{label}</text>
      {value ? <text style={{ ...TYPE.caption, color: C.secondary }}>{value}</text> : null}
    </div>
  )
}

export function InfoPanel({ chat }: { chat: Chat }) {
  const shell = useShell()
  const { store } = shell
  const state = useAppState(store)
  const [name, setName] = useState(chat.displayName ?? '')
  const [address, setAddress] = useState('')
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [confirmLeave, setConfirmLeave] = useState(false)
  const manage = state.capabilities.groupManagement && chat.isGroup

  return (
    <div testId="info-panel" style={{ display: 'flex', flexDirection: 'column', width: 300, flexShrink: 0, height: '100%', backgroundColor: C.sidebar, borderLeftWidth: 1, borderColor: C.sidebarBorder, overflowY: 'scroll', userSelect: 'none' }}>
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', height: TITLEBAR_HEIGHT, paddingLeft: 16, paddingRight: 8 }}>
        <text style={{ ...TYPE.title, color: C.text, flexGrow: 1 }}>Details</text>
        <IconButton icon="close" label="Close details" onClick={shell.toggleInfo} />
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 8, paddingTop: 8, paddingBottom: 16 }}>
        <Avatar chat={chat} size={80} />
        <text style={{ ...TYPE.large, fontSize: 18, lineHeight: 24, color: C.text, textAlign: 'center', paddingLeft: 16, paddingRight: 16 }}>{chatTitle(chat)}</text>
        <text style={{ ...TYPE.caption, color: C.secondary }}>{chat.isGroup ? `${chat.participants.length} people · ${chat.service}` : chat.service}</text>
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', paddingLeft: 8, paddingRight: 8, gap: 2 }}>
        <Row icon={chat.pinned ? 'pinOff' : 'pin'} label={chat.pinned ? 'Unpin' : 'Pin'} testId="toggle-pin" onClick={() => store.togglePin(chat.guid)} />
        <Row icon={chat.muted ? 'unmute' : 'mute'} label={chat.muted ? 'Show alerts' : 'Hide alerts'} testId="toggle-mute" onClick={() => store.toggleMute(chat.guid)} />
        <Row icon="markUnread" label="Mark as unread" onClick={() => void store.markUnread(chat.guid)} />
      </div>
      <div style={{ paddingTop: 12, paddingBottom: 12, paddingLeft: 16, paddingRight: 16 }}>
        <Divider />
      </div>
      <text style={{ ...TYPE.micro, fontWeight: 600, color: C.tertiary, paddingLeft: 18, paddingBottom: 6 }}>{chat.isGroup ? 'People' : 'Contact'}</text>
      <div style={{ display: 'flex', flexDirection: 'column', paddingLeft: 8, paddingRight: 8 }}>
        {chat.participants.map((handle) => (
          <div key={handle.address} style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10, height: 44, paddingLeft: 10, paddingRight: 6, borderRadius: RADIUS.control, hover: manage ? { backgroundColor: C.raised } : undefined }}>
            <Avatar handle={handle} size={30} />
            <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minWidth: 0 }}>
              <text style={{ ...TYPE.body, color: C.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{handleName(handle)}</text>
              {handle.name ? <text style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{formatAddress(handle.address)}</text> : null}
            </div>
            {manage && chat.participants.length > 2 ? <IconButton icon="close" label={`Remove ${handleName(handle)}`} size={12} hit={24} onClick={() => void store.transport.removeParticipant(chat.guid, handle.address)} /> : null}
          </div>
        ))}
      </div>
      {manage ? (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8, paddingTop: 12, paddingLeft: 16, paddingRight: 16 }}>
          <div style={{ display: 'flex', flexDirection: 'row', gap: 6 }}>
            <TextField value={address} onChange={setAddress} placeholder="Phone number or email" onSubmit={() => void store.transport.addParticipant(chat.guid, address.trim()).then(() => setAddress(''))} />
            <Button onClick={() => void store.transport.addParticipant(chat.guid, address.trim()).then(() => setAddress(''))} disabled={address.trim().length === 0}>
              Add
            </Button>
          </div>
          <div style={{ display: 'flex', flexDirection: 'row', gap: 6 }}>
            <TextField value={name} onChange={setName} placeholder="Group name" onSubmit={() => void store.renameGroup(chat.guid, name.trim())} />
            <Button onClick={() => void store.renameGroup(chat.guid, name.trim())} disabled={name.trim() === (chat.displayName ?? '')}>
              Rename
            </Button>
          </div>
        </div>
      ) : null}
      <div style={{ paddingTop: 12, paddingBottom: 12, paddingLeft: 16, paddingRight: 16 }}>
        <Divider />
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', paddingLeft: 8, paddingRight: 8, gap: 2, paddingBottom: 16 }}>
        {chat.isGroup && state.capabilities.groupManagement ? (
          confirmLeave ? (
            <Row icon="leave" label="Leave conversation, are you sure?" danger onClick={() => void store.leaveGroup(chat.guid).then(() => setConfirmLeave(false))} />
          ) : (
            <Row icon="leave" label="Leave conversation" danger onClick={() => setConfirmLeave(true)} />
          )
        ) : null}
        {confirmDelete ? (
          <Row icon="trash" label="Delete conversation, are you sure?" danger testId="confirm-delete" onClick={() => void store.deleteChat(chat.guid)} />
        ) : (
          <Row icon="trash" label="Delete conversation" danger testId="delete-chat" onClick={() => setConfirmDelete(true)} />
        )}
      </div>
    </div>
  )
}
