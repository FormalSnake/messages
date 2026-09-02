import { useState } from 'react'
import type { ServerInfo } from '@messages/core'
import { C, RADIUS, TYPE } from './theme'
import { Icon } from './icons'
import { Button, Divider, IconButton, TextField } from './primitives'

export function ConnectScreen({
  initialUrl,
  initialPassword,
  error,
  connecting,
  server,
  onConnect,
  onDemo,
  onClose,
}: {
  initialUrl: string
  initialPassword: string
  error?: string
  connecting: boolean
  server: ServerInfo | null
  onConnect: (url: string, password: string) => void
  onDemo: () => void
  onClose?: () => void
}) {
  const [url, setUrl] = useState(initialUrl)
  const [password, setPassword] = useState(initialPassword)
  const valid = /^https?:\/\/\S+$/.test(url.trim()) && password.length > 0
  const submit = () => {
    if (valid && !connecting) onConnect(url.trim().replace(/\/+$/, ''), password)
  }
  const privateApiOn = Boolean(server?.privateApi && server.helperConnected)

  return (
    <div testId="connect" style={{ flexGrow: 1, height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', backgroundColor: C.canvas, pointerEvents: 'auto' }}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 14, width: 420, padding: 24, borderRadius: RADIUS.card, backgroundColor: C.sidebar, borderWidth: 1, borderColor: C.sidebarBorder }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 8 }}>
          <text style={{ ...TYPE.large, color: C.text, flexGrow: 1 }}>Connect to your Mac</text>
          {onClose ? <IconButton icon="close" label="Close settings" testId="close-settings" onClick={onClose} /> : null}
        </div>
        <text style={{ ...TYPE.body, color: C.secondary }}>Messages talks to the BlueBubbles server running on your Mac. Copy the server address and password from its settings.</text>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <text style={{ ...TYPE.caption, fontWeight: 600, color: C.secondary }}>Server address</text>
          <TextField testId="server-url" value={url} onChange={setUrl} onSubmit={submit} placeholder="http://192.168.1.20:1234" autoFocus={!initialUrl} />
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <text style={{ ...TYPE.caption, fontWeight: 600, color: C.secondary }}>Server password</text>
          <TextField testId="server-password" value={password} onChange={setPassword} onSubmit={submit} placeholder="Password" secure autoFocus={Boolean(initialUrl)} />
        </div>
        {error ? (
          <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-start', gap: 8, padding: 10, borderRadius: RADIUS.control, backgroundColor: '#ff453a1f' }}>
            <Icon name="alert" size={14} color={C.danger} />
            <text testId="connect-error" style={{ ...TYPE.caption, color: C.text, flexGrow: 1, minWidth: 0 }}>{error}</text>
          </div>
        ) : null}
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 8 }}>
          <Button kind="primary" testId="connect-button" onClick={submit} disabled={!valid || connecting}>
            {connecting ? 'Connecting…' : 'Connect'}
          </Button>
          <Button testId="demo-button" onClick={onDemo}>Use demo data</Button>
        </div>
        <Divider />
        {server ? (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            <text style={{ ...TYPE.caption, fontWeight: 600, color: C.secondary }}>Connected server</text>
            <Line label="BlueBubbles" value={server.version} />
            <Line label="macOS" value={server.macosVersion ?? 'unknown'} />
            <Line label="iCloud" value={server.icloudAccount ?? 'not detected'} />
            <Line label="Private API" value={privateApiOn ? 'On' : server.privateApi ? 'On, helper not connected' : 'Off'} ok={privateApiOn} />
          </div>
        ) : null}
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-start', gap: 8 }}>
          <Icon name={privateApiOn ? 'unlock' : 'lock'} size={14} color={C.tertiary} />
          <text style={{ ...TYPE.caption, color: C.tertiary, flexGrow: 1, minWidth: 0 }}>
            Tapbacks, typing indicators, read receipts, replies, edits and effects need the Private API. Turn it on in BlueBubbles on a Mac with SIP disabled. Everything else works without it.
          </text>
        </div>
      </div>
    </div>
  )
}

function Line({ label, value, ok }: { label: string; value: string; ok?: boolean }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 8 }}>
      <text style={{ ...TYPE.caption, color: C.secondary, width: 90 }}>{label}</text>
      {ok !== undefined ? <div style={{ width: 7, height: 7, borderRadius: 4, backgroundColor: ok ? C.online : C.warning }} /> : null}
      <text style={{ ...TYPE.caption, color: C.text }}>{value}</text>
    </div>
  )
}
