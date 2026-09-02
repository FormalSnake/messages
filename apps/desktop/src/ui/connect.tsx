import { useState } from 'react'
import type { ServerInfo } from '@messages/core'
import { C, RADIUS, S, TYPE } from './theme'
import { Icon } from './icons'
import { Button, Divider, IconButton, TextField } from './primitives'

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: S.x1 }}>
      <text style={{ ...TYPE.caption, fontWeight: 600, color: C.secondary }}>{label}</text>
      {children}
    </div>
  )
}

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
    <div
      testId="connect"
      style={{
        flexGrow: 1,
        height: '100%',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: S.x6,
        backgroundColor: C.canvas,
        pointerEvents: 'auto',
      }}
    >
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: S.x4,
          width: 420,
          maxWidth: '100%',
          padding: S.x6,
          borderRadius: RADIUS.card,
          backgroundColor: C.sidebar,
          borderWidth: 1,
          borderColor: C.sidebarBorder,
        }}
      >
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-start', gap: S.x2 }}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: S.x1, flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0 }}>
            <text style={{ ...TYPE.large, color: C.text }}>Connect to your Mac</text>
            <text style={{ ...TYPE.body, color: C.secondary }}>
              Messages talks to the BlueBubbles server running on your Mac. Copy the address and password from its settings.
            </text>
          </div>
          {onClose ? <IconButton icon="close" label="Close settings (Esc)" testId="close-settings" onClick={onClose} /> : null}
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', gap: S.x3 }}>
          <Field label="Server address">
            <TextField testId="server-url" value={url} onChange={setUrl} onSubmit={submit} placeholder="http://192.168.1.20:1234" autoFocus={!initialUrl} />
          </Field>
          <Field label="Server password">
            <TextField testId="server-password" value={password} onChange={setPassword} onSubmit={submit} placeholder="Password" secure autoFocus={Boolean(initialUrl)} />
          </Field>
        </div>

        {error ? (
          <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-start', gap: S.x2, padding: S.x3, borderRadius: RADIUS.control, backgroundColor: C.dangerSoft }}>
            <Icon name="alert" size={14} color={C.danger} />
            <text testId="connect-error" style={{ ...TYPE.caption, color: C.text, flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0 }}>
              {error}
            </text>
          </div>
        ) : null}

        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x2 }}>
          <Button kind="primary" testId="connect-button" onClick={submit} disabled={!valid || connecting}>
            {connecting ? 'Connecting…' : 'Connect'}
          </Button>
          <Button testId="demo-button" onClick={onDemo}>
            Use demo data
          </Button>
        </div>

        <Divider />

        {server ? (
          <div style={{ display: 'flex', flexDirection: 'column', gap: S.x2 }}>
            <text style={{ ...TYPE.caption, fontWeight: 600, color: C.secondary }}>Connected server</text>
            <Line label="BlueBubbles" value={server.version} />
            <Line label="macOS" value={server.macosVersion ?? 'unknown'} />
            <Line label="iCloud" value={server.icloudAccount ?? 'not detected'} />
            <Line label="Private API" value={privateApiOn ? 'On' : server.privateApi ? 'On, helper not connected' : 'Off'} ok={privateApiOn} />
          </div>
        ) : null}

        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-start', gap: S.x2 }}>
          <Icon name={privateApiOn ? 'unlock' : 'lock'} size={14} color={C.tertiary} />
          <text style={{ ...TYPE.caption, color: C.tertiary, flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0 }}>
            Tapbacks, typing indicators, read receipts, replies, edits and effects need the Private API. Turn it on in BlueBubbles on a Mac with SIP disabled. Everything else
            works without it.
          </text>
        </div>
      </div>
    </div>
  )
}

function Line({ label, value, ok }: { label: string; value: string; ok?: boolean }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x2 }}>
      <text style={{ ...TYPE.caption, color: C.secondary, width: 88, flexShrink: 0 }}>{label}</text>
      {ok !== undefined ? <div style={{ width: 7, height: 7, borderRadius: 4, backgroundColor: ok ? C.online : C.warning, flexShrink: 0 }} /> : null}
      <text style={{ ...TYPE.caption, color: C.text, flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{value}</text>
    </div>
  )
}
