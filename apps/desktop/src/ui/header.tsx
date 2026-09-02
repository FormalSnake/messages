import { useState } from 'react'
import { chatTitle, copyText, formatAddress, handleName, openExternal, type Chat, type Handle } from '@messages/core'
import { C, INFO_WIDTH, RADIUS, S, TITLEBAR_HEIGHT, TYPE } from './theme'
import { Icon, type IconName } from './icons'
import { Avatar, Button, Divider, IconButton, SectionLabel, TextField } from './primitives'
import { shortcut, useShell, type MenuItem } from './context'
import { chatMenu } from './sidebar'
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
        <Avatar chat={chat} size={30} surface={C.canvas} />
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
      <Avatar handle={handle} size={28} surface={C.sidebar} />
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

export function InfoPanel({ chat, floating }: { chat: Chat; floating: boolean }) {
  const shell = useShell()
  const { store } = shell
  const state = useAppState(store)
  const [name, setName] = useState(chat.displayName ?? '')
  const [address, setAddress] = useState('')
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [confirmLeave, setConfirmLeave] = useState(false)
  const manage = state.capabilities.groupManagement && chat.isGroup

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
        <Avatar chat={chat} size={72} surface={C.sidebar} />
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
        {chat.participants.map((handle) => (
          <Participant key={handle.address} handle={handle} chat={chat} manage={manage} />
        ))}
      </div>

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
        {chat.isGroup && state.capabilities.groupManagement ? (
          <Row
            icon="leave"
            label={confirmLeave ? 'Leave for good?' : 'Leave conversation'}
            danger
            onClick={confirmLeave ? () => void store.leaveGroup(chat.guid).then(() => setConfirmLeave(false)) : () => setConfirmLeave(true)}
          />
        ) : null}
        <Row
          icon="trash"
          label={confirmDelete ? 'Delete for good?' : 'Delete conversation'}
          danger
          testId={confirmDelete ? 'confirm-delete' : 'delete-chat'}
          onClick={confirmDelete ? () => void store.deleteChat(chat.guid) : () => setConfirmDelete(true)}
        />
      </div>
    </div>
  )
}
