import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { TooltipProvider, useGpuix, useWindowInsets, type PublicInstance } from '@gpuix/react'
import {
  BlueBubblesTransport,
  DemoTransport,
  MessagesStore,
  attachmentsDir,
  notifyIncoming,
  openExternal,
  type Config,
  type ServerConfig,
  type Transport,
} from '@messages/core'
import { C, FONT_SANS, TYPE } from './theme'
import { Icon } from './icons'
import { Button, IconButton } from './primitives'
import { ShellContext, primaryModifier, type MenuRequest, type Shell } from './context'
import { useAppState } from './use-app-state'
import { Sidebar } from './sidebar'
import { Thread } from './thread'
import { Composer } from './composer'
import { ConversationHeader, InfoPanel } from './header'
import { ContextMenu } from './menus'
import { ConnectScreen } from './connect'
import { NewChat } from './new-chat'

export interface MessagesAppProps {
  config: Config
  saveConfig: (patch: Partial<Config>) => Promise<unknown>
  /** Test hook: supply a transport instead of building one from the config. */
  transport?: Transport
}

function buildTransport(config: Config): Transport | null {
  if (config.demo) return new DemoTransport()
  if (config.server) return new BlueBubblesTransport({ url: config.server.url, password: config.server.password, attachmentsDir })
  return null
}

function useStore(config: Config, override: Transport | undefined, saveConfig: MessagesAppProps['saveConfig']): MessagesStore | null {
  const store = useMemo(() => {
    const transport = override ?? buildTransport(config)
    if (!transport) return null
    return new MessagesStore(transport, {
      prefs: config.chats,
      onPrefsChange: (chats) => void saveConfig({ chats }),
      onIncoming: config.notifications ? (chat, message) => void notifyIncoming(chat, message) : undefined,
    })
  }, [config, override, saveConfig])
  useEffect(() => {
    if (!store) return
    void store.start()
    return () => store.stop()
  }, [store])
  return store
}

export function MessagesApp({ config: initialConfig, saveConfig, transport }: MessagesAppProps) {
  const [config, setConfig] = useState(initialConfig)
  const persist = useCallback(
    async (patch: Partial<Config>) => {
      const next = { ...config, ...patch }
      setConfig(next)
      await saveConfig(patch)
    },
    [config, saveConfig],
  )
  const store = useStore(config, transport, persist)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const connectTo = (server: ServerConfig) => {
    setSettingsOpen(false)
    void persist({ server, demo: false })
  }
  const useDemo = () => {
    setSettingsOpen(false)
    void persist({ demo: true })
  }

  if (!store) {
    return (
      <Frame>
        <ConnectScreen initialUrl="" initialPassword="" connecting={false} server={null} onConnect={(url, password) => connectTo({ url, password })} onDemo={useDemo} />
      </Frame>
    )
  }
  return (
    <Frame>
      <Workspace key={store === null ? 'none' : config.demo ? 'demo' : config.server?.url} store={store} config={config} settingsOpen={settingsOpen} setSettingsOpen={setSettingsOpen} onConnect={connectTo} onDemo={useDemo} />
    </Frame>
  )
}

function Frame({ children }: { children: React.ReactNode }) {
  return (
    <TooltipProvider delayDuration={500}>
      <div testId="app" style={{ display: 'flex', flexDirection: 'row', width: '100%', height: '100%', backgroundColor: C.canvas, fontFamily: FONT_SANS, color: C.text }}>
        {children}
      </div>
    </TooltipProvider>
  )
}

function Workspace({
  store,
  config,
  settingsOpen,
  setSettingsOpen,
  onConnect,
  onDemo,
}: {
  store: MessagesStore
  config: Config
  settingsOpen: boolean
  setSettingsOpen: (open: boolean) => void
  onConnect: (server: ServerConfig) => void
  onDemo: () => void
}) {
  const state = useAppState(store)
  const { renderer } = useGpuix()
  const { ime } = useWindowInsets()
  const [menu, setMenu] = useState<MenuRequest | null>(null)
  const [newChat, setNewChat] = useState(false)
  const [infoOpen, setInfoOpen] = useState(false)
  const [toast, setToast] = useState<string | null>(null)
  const searchRef = useRef<PublicInstance | null>(null)
  const selected = state.chats.find((chat) => chat.guid === state.selectedChat) ?? null

  useEffect(() => {
    if (!state.error || settingsOpen) return
    setToast(state.error)
    const timer = setTimeout(() => {
      setToast(null)
      store.clearError()
    }, 6000)
    return () => clearTimeout(timer)
  }, [state.error, settingsOpen, store])

  const shell = useMemo<Shell>(
    () => ({
      store,
      openMenu: setMenu,
      closeMenu: () => setMenu(null),
      openSettings: () => setSettingsOpen(true),
      startNewChat: () => {
        setNewChat(true)
        setInfoOpen(false)
      },
      toggleInfo: () => setInfoOpen((open) => !open),
      focusSearch: () => {
        if (searchRef.current && renderer?.focusElement) renderer.focusElement(searchRef.current.id)
      },
    }),
    [store, renderer, setSettingsOpen],
  )

  const stepChat = (delta: number) => {
    if (state.chats.length === 0) return
    const index = state.chats.findIndex((chat) => chat.guid === state.selectedChat)
    const next = state.chats[(index + delta + state.chats.length) % state.chats.length]
    if (next) {
      setNewChat(false)
      void store.selectChat(next.guid)
    }
  }

  const showConnect = settingsOpen || (state.status === 'offline' && state.chats.length === 0 && store.transport.kind !== 'demo')

  return (
    <ShellContext.Provider value={shell}>
      <div
        tabIndex={-1}
        onKeyDown={(event) => {
          const primary = primaryModifier(event.modifiers)
          if (event.key === 'escape') {
            if (menu) setMenu(null)
            else if (newChat) setNewChat(false)
            else if (infoOpen) setInfoOpen(false)
            else if (settingsOpen) setSettingsOpen(false)
            return
          }
          if (!primary) return
          if (event.key === 'n') shell.startNewChat()
          else if (event.key === 'f') shell.focusSearch()
          else if (event.key === 'i') shell.toggleInfo()
          else if (event.key === ',') setSettingsOpen(true)
          else if (event.key === ']' || (event.modifiers?.shift && event.key === 'down')) stepChat(1)
          else if (event.key === '[' || (event.modifiers?.shift && event.key === 'up')) stepChat(-1)
        }}
        style={{ display: 'flex', flexDirection: 'row', width: '100%', height: '100%', position: 'relative', paddingBottom: ime.bottom }}
      >
        <Sidebar searchRef={searchRef} />
        <div style={{ width: 1, height: '100%', flexShrink: 0, backgroundColor: C.sidebarBorder }} />
        <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minWidth: 0, height: '100%', backgroundColor: C.canvas }}>
          {newChat ? (
            <NewChat onClose={() => setNewChat(false)} />
          ) : selected ? (
            <>
              <ConversationHeader chat={selected} infoOpen={infoOpen} />
              <Thread chat={selected} />
              <Composer chat={selected} />
            </>
          ) : (
            <EmptyState status={state.status} />
          )}
        </div>
        {infoOpen && selected && !newChat ? <InfoPanel chat={selected} /> : null}

        {menu ? <ContextMenu request={menu} /> : null}

        {state.facetime ? (
          <div style={{ position: 'absolute', top: 12, right: 12, display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10, padding: 12, borderRadius: 12, backgroundColor: C.overlay, borderWidth: 1, borderColor: C.overlayBorder, pointerEvents: 'auto', boxShadow: { offsetX: 0, offsetY: 8, blurRadius: 24, spreadRadius: 0, color: '#00000080' } }}>
            <Icon name="video" size={18} color={C.online} />
            <div style={{ display: 'flex', flexDirection: 'column' }}>
              <text style={{ ...TYPE.body, fontWeight: 600, color: C.text }}>FaceTime</text>
              <text style={{ ...TYPE.caption, color: C.secondary }}>{state.facetime.from ?? 'Incoming call'}</text>
            </div>
            {state.facetime.link ? <Button kind="primary" onClick={() => openExternal(state.facetime!.link!)}>Join in browser</Button> : null}
            <IconButton icon="close" label="Dismiss" onClick={() => store.dismissFaceTime()} />
          </div>
        ) : null}

        {toast && !showConnect ? (
          <div testId="toast" style={{ position: 'absolute', bottom: 64, left: 0, right: 0, display: 'flex', alignItems: 'center', pointerEvents: 'none' }}>
            <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 8, paddingLeft: 14, paddingRight: 14, paddingTop: 8, paddingBottom: 8, borderRadius: 10, backgroundColor: C.overlay, borderWidth: 1, borderColor: C.overlayBorder, maxWidth: 520 }}>
              <Icon name="alert" size={14} color={C.danger} />
              <text style={{ ...TYPE.caption, color: C.text }}>{toast}</text>
            </div>
          </div>
        ) : null}

        {showConnect ? (
          <div style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, display: 'flex', pointerEvents: 'auto', backgroundColor: C.canvas }}>
            <ConnectScreen
              initialUrl={config.server?.url ?? ''}
              initialPassword={config.server?.password ?? ''}
              error={state.status === 'offline' ? state.error : undefined}
              connecting={state.status === 'connecting'}
              server={state.server}
              onConnect={(url, password) => onConnect({ url, password })}
              onDemo={onDemo}
              onClose={settingsOpen && (state.chats.length > 0 || store.transport.kind === 'demo') ? () => setSettingsOpen(false) : undefined}
            />
          </div>
        ) : null}
      </div>
    </ShellContext.Provider>
  )
}

function EmptyState({ status }: { status: string }) {
  return (
    <div style={{ flexGrow: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 6 }}>
      <text style={{ ...TYPE.title, color: C.text }}>{status === 'online' ? 'No conversation selected' : 'Connecting to your Mac…'}</text>
      <text style={{ ...TYPE.caption, color: C.secondary }}>{status === 'online' ? 'Pick one on the left or press Ctrl+N.' : 'Conversations appear once the server answers.'}</text>
    </div>
  )
}
