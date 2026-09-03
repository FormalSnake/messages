import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { TooltipProvider, useGpuix, useWindowInsets, useWindowSize, type PublicInstance } from '@gpuix/react'
import {
  BlueBubblesTransport,
  CanaryLLMClient,
  DemoTransport,
  KlipyClient,
  MessagesStore,
  attachmentsDir,
  notifyIncoming,
  openExternal,
  type Config,
  type Message,
  type ServerConfig,
  type Transport,
  StateCache,
  cacheDir,
  conversationChats,
  conversationGuid,
} from '@messages/core'
import { C, FONT_SANS, RADIUS, S, SIDEBAR_WIDTH, SIDEBAR_WIDTH_COMPACT, INFO_WIDTH, TYPE } from './theme'
import { Icon } from './icons'
import { Button, IconButton, overlayShadow } from './primitives'
import { ShellContext, primaryModifier, shortcut, type MenuRequest, type Shell } from './context'
import { useAppState } from './use-app-state'
import { Sidebar } from './sidebar'
import { Thread } from './thread'
import { Composer } from './composer'
import { ConversationHeader, InfoPanel } from './header'
import { ContextMenu, TAPBACK_ORDER } from './menus'
import { ConnectScreen } from './connect'
import { NewChat } from './new-chat'
import { FaceTimeBanner } from './facetime'
import { Lightbox } from './lightbox'
import { ConfirmDialog, type ConfirmRequest } from './confirm'
import { watchTheme } from './live-theme'
import { Switcher } from './switcher'

export interface MessagesAppProps {
  config: Config
  saveConfig: (patch: Partial<Config>) => Promise<unknown>
  /** Test hook: supply a transport instead of building one from the config. */
  transport?: Transport
}

/** Below this the thread would be narrower than a comfortable measure, so the details panel floats over it. */
const DOCKED_INFO_MIN_WIDTH = 1000
const COMPACT_SIDEBAR_MAX_WIDTH = 900

function buildTransport(config: Config): Transport | null {
  if (config.demo) return new DemoTransport()
  if (config.server) return new BlueBubblesTransport({ url: config.server.url, password: config.server.password, attachmentsDir })
  return null
}

function connectionKey(config: Config): string {
  if (config.demo) return 'demo'
  return config.server ? `${config.server.url}\u0000${config.server.password}\u0000${config.agent?.url ?? ''}` : 'none'
}

/** One store per connection. Preference edits (pins, mutes, notifications) must not reconnect. */
function useStore(config: Config, override: Transport | undefined, saveConfig: MessagesAppProps["saveConfig"]): { store: MessagesStore | null; setActivate: (activate: () => void) => void } {
  const latest = useRef<{ config: Config; saveConfig: MessagesAppProps['saveConfig']; store: MessagesStore | null; activate: () => void }>({ config, saveConfig, store: null, activate: () => undefined })
  latest.current.config = config
  latest.current.saveConfig = saveConfig
  const key = override ? 'override' : connectionKey(config)
  const store = useMemo(() => {
    const current = latest.current.config
    const transport = override ?? buildTransport(current)
    if (!transport) return null
    return new MessagesStore(transport, {
      prefs: current.chats,
      agent: current.agent,
      cache: current.demo ? undefined : new StateCache(cacheDir),
      onPrefsChange: (chats) => void latest.current.saveConfig({ chats }),
      onIncoming: (chat, message, target) => {
        if (!latest.current.config.notifications) return
        void notifyIncoming(chat, message, { target, icon: NOTIFICATION_ICON }).then((action) => {
          if (action !== 'open') return
          void latest.current.store?.selectChat(chat.guid)  // the store folds a member into its conversation
          latest.current.activate()
        })
      },
    })
  }, [key, override])
  latest.current.store = store
  useEffect(() => {
    if (!store) return
    void store.start()
    return () => store.stop()
  }, [store])
  return {
    store,
    setActivate: (activate: () => void) => {
      latest.current.activate = activate
    },
  }
}

const NOTIFICATION_ICON = new URL('../../assets/icon.svg', import.meta.url).pathname

/** `config.canaryllm.language`, else the system locale's language written out in English, else English. */
function defaultTranslateLanguage(configured: string | undefined): string {
  if (configured?.trim()) return configured.trim()
  try {
    const locale = Intl.DateTimeFormat().resolvedOptions().locale
    const language = new Intl.DisplayNames(['en'], { type: 'language' }).of(locale.split('-')[0] ?? locale)
    return language ?? 'English'
  } catch {
    return 'English'
  }
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
  const { store, setActivate } = useStore(config, transport, persist)
  const [settingsOpen, setSettingsOpen] = useState(false)
  // The palette is swapped in place, so a theme change remounts the tree to repaint every style object.
  const [themeVersion, setThemeVersion] = useState(0)
  useEffect(() => watchTheme(() => setThemeVersion((version) => version + 1)), [])
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
      <Frame key={themeVersion}>
        <ConnectScreen initialUrl="" initialPassword="" connecting={false} server={null} onConnect={(url, password) => connectTo({ url, password })} onDemo={useDemo} />
      </Frame>
    )
  }
  return (
    <Frame key={themeVersion}>
      <Workspace setActivate={setActivate}
        key={store === null ? 'none' : config.demo ? 'demo' : config.server?.url}
        store={store}
        config={config}
        settingsOpen={settingsOpen}
        setSettingsOpen={setSettingsOpen}
        onConnect={connectTo}
        onDemo={useDemo}
      />
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
  setActivate,
  settingsOpen,
  setSettingsOpen,
  onConnect,
  onDemo,
}: {
  store: MessagesStore
  config: Config
  setActivate: (activate: () => void) => void
  settingsOpen: boolean
  setSettingsOpen: (open: boolean) => void
  onConnect: (server: ServerConfig) => void
  onDemo: () => void
}) {
  const state = useAppState(store)
  const { renderer } = useGpuix()
  useEffect(() => {
    setActivate(() => renderer?.activateWindow?.())
  }, [renderer, setActivate])
  const { ime } = useWindowInsets()
  const { width } = useWindowSize()
  const [menu, setMenu] = useState<MenuRequest | null>(null)
  const [newChat, setNewChat] = useState(false)
  const [infoOpen, setInfoOpen] = useState(false)
  const [switcherOpen, setSwitcherOpen] = useState(false)
  const [toast, setToast] = useState<string | null>(null)
  const [lightbox, setLightbox] = useState<{ chatGuid: string; attachmentGuid: string } | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const searchRef = useRef<PublicInstance | null>(null)
  const gifs = useMemo(() => (config.klipy ? new KlipyClient(config.klipy) : null), [config.klipy?.apiKey])
  const assistant = useMemo(
    () => (config.canaryllm?.apiKey ? new CanaryLLMClient({ apiKey: config.canaryllm.apiKey, model: config.canaryllm.model }) : null),
    [config.canaryllm?.apiKey, config.canaryllm?.model],
  )
  const assistantLanguage = useMemo(() => defaultTranslateLanguage(config.canaryllm?.language), [config.canaryllm?.language])
  const selected = state.chats.find((chat) => chat.guid === state.selectedChat) ?? null
  const sidebarWidth = width > 0 && width < COMPACT_SIDEBAR_MAX_WIDTH ? SIDEBAR_WIDTH_COMPACT : SIDEBAR_WIDTH
  const infoFloats = width > 0 && width < DOCKED_INFO_MIN_WIDTH

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
      gifs,
      openMenu: setMenu,
      closeMenu: () => setMenu(null),
      openSettings: () => setSettingsOpen(true),
      startNewChat: () => {
        setMenu(null)
        setNewChat(true)
        setInfoOpen(false)
      },
      openSwitcher: () => {
        setMenu(null)
        setSwitcherOpen(true)
      },
      toggleInfo: () => {
        setMenu(null)
        setInfoOpen((open) => !open)
      },
      setInfo: (open: boolean) => {
        setMenu(null)
        setInfoOpen(open)
      },
      focusSearch: () => {
        if (searchRef.current && renderer?.focusElement) renderer.focusElement(searchRef.current.id)
      },
      openLightbox: (target) => {
        setMenu(null)
        setLightbox(target)
      },
      closeLightbox: () => setLightbox(null),
      confirm: (request) => {
        setMenu(null)
        setConfirm(request)
      },
      assistant,
      assistantLanguage,
    }),
    [store, gifs, renderer, setSettingsOpen, assistant, assistantLanguage],
  )

  const stepChat = (delta: number) => {
    const rows = conversationChats(state)
    if (rows.length === 0) return
    const index = rows.findIndex((chat) => chat.guid === state.selectedChat)
    const next = rows[(index + delta + rows.length) % rows.length]
    if (next) {
      setNewChat(false)
      void store.selectChat(next.guid)
    }
  }

  /** The newest bubble from the other side, for a Ctrl+1..6 tapback. Skips group events and unsends, which never render as reactable bubbles. */
  const lastIncomingReactable = (messages: Message[] | undefined): Message | undefined => {
    if (!messages) return undefined
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      const message = messages[index]
      if (message && !message.fromMe && !message.groupEvent && !message.dateRetracted) return message
    }
    return undefined
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
            else if (confirm) setConfirm(null)
            else if (switcherOpen) setSwitcherOpen(false)
            else if (newChat) setNewChat(false)
            else if (infoOpen) setInfoOpen(false)
            else if (settingsOpen) setSettingsOpen(false)
            return
          }
          if (!primary) return
          if (event.key === 'n') shell.startNewChat()
          else if (event.key === 'k') shell.openSwitcher()
          else if (event.key === 'f') shell.focusSearch()
          else if (event.key === 'i') shell.toggleInfo()
          else if (event.key === ',') setSettingsOpen(true)
          else if (event.key === 'u' && event.modifiers?.shift) {
            if (state.selectedChat) void store.markUnread(state.selectedChat)
          } else if (event.key === ']' || (event.modifiers?.shift && event.key === 'down')) stepChat(1)
          else if (event.key === '[' || (event.modifiers?.shift && event.key === 'up')) stepChat(-1)
          else if (typeof event.key === 'string' && event.key.length === 1 && event.key >= '1' && event.key <= '6' && state.capabilities.reactions && selected) {
            const target = lastIncomingReactable(state.messages[selected.guid])
            const kind = TAPBACK_ORDER[Number(event.key) - 1]
            if (target && kind) void store.react(selected.guid, target.guid, kind)
          }
        }}
        style={{ display: 'flex', flexDirection: 'row', width: '100%', height: '100%', position: 'relative', paddingBottom: ime.bottom }}
      >
        <Sidebar searchRef={searchRef} width={sidebarWidth} />
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
            <EmptyState status={state.status} reason={state.connectionError} onNew={shell.startNewChat} />
          )}
        </div>
        {infoOpen && selected && !newChat ? <InfoPanel chat={selected} floating={infoFloats} /> : null}

        {menu ? <ContextMenu request={menu} /> : null}

        {confirm ? <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} /> : null}
        {switcherOpen ? <Switcher onClose={() => setSwitcherOpen(false)} /> : null}

        {lightbox ? <Lightbox target={lightbox} /> : null}

        <FaceTimeBanner offsetRight={infoOpen && !infoFloats ? INFO_WIDTH + S.x3 : S.x3} />

        {toast && !showConnect ? (
          <div
            testId="toast"
            style={{ position: 'absolute', bottom: 72, left: 0, right: 0, display: 'flex', flexDirection: 'row', justifyContent: 'center', alignItems: 'center', pointerEvents: 'none' }}
          >
            <div
              style={{
                display: 'flex',
                flexDirection: 'row',
                alignItems: 'center',
                gap: S.x2,
                paddingLeft: S.x3,
                paddingRight: S.x3,
                paddingTop: S.x2,
                paddingBottom: S.x2,
                borderRadius: RADIUS.menu,
                backgroundColor: C.overlay,
                borderWidth: 1,
                borderColor: C.overlayBorder,
                boxShadow: overlayShadow,
                maxWidth: 480,
              }}
            >
              <Icon name="alert" size={14} color={C.danger} />
              <text style={{ ...TYPE.caption, color: C.text, flexShrink: 1, minWidth: 0 }}>{toast}</text>
            </div>
          </div>
        ) : null}

        {showConnect ? (
          <div style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, display: 'flex', pointerEvents: 'auto', backgroundColor: C.canvas }}>
            <ConnectScreen
              initialUrl={config.server?.url ?? ''}
              initialPassword={config.server?.password ?? ''}
              error={state.status === 'offline' ? state.connectionError : undefined}
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

function EmptyState({ status, reason, onNew }: { status: string; reason?: string; onNew: () => void }) {
  const online = status === 'online'
  const title = online ? 'No conversation selected' : status === 'connecting' ? 'Connecting to your Mac…' : 'Your Mac is not answering'
  const body = online
    ? 'Pick one on the left, or start a new one.'
    : status === 'connecting'
      ? 'Conversations appear once the server answers.'
      : `Retrying. ${reason ? `Last attempt: ${reason}.` : 'Check that BlueBubbles is running and reachable.'}`
  return (
    <div style={{ flexGrow: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: S.x2, paddingLeft: S.x6, paddingRight: S.x6 }}>
      <Icon name="conversation" size={30} color={C.tertiary} />
      <text style={{ ...TYPE.title, fontSize: 15, color: C.text, textAlign: 'center' }}>{title}</text>
      <text style={{ ...TYPE.caption, color: C.secondary, textAlign: 'center' }}>{body}</text>
      {online ? (
        <div style={{ paddingTop: S.x2 }}>
          <Button kind="primary" onClick={onNew}>{`New message  ${shortcut('N')}`}</Button>
        </div>
      ) : null}
    </div>
  )
}
