import { useEffect, useRef, useState } from 'react'
import { Select, SelectContent, SelectItem, SelectTrigger, useGpuix, type PublicInstance } from '@gpuix/react'
import { clipboardImage, handleName, type Chat } from '@messages/core'
import { C, RADIUS, S, TYPE } from './theme'
import { Icon } from './icons'
import { IconButton, TextField, overlayShadow } from './primitives'
import { primaryModifier, useShell } from './context'
import { useAppState } from './use-app-state'
import { effectName } from './thread'

const EFFECTS = [
  'com.apple.MobileSMS.expressivesend.impact',
  'com.apple.MobileSMS.expressivesend.loud',
  'com.apple.MobileSMS.expressivesend.gentle',
  'com.apple.MobileSMS.expressivesend.invisibleink',
  'com.apple.messages.effect.CKEchoEffect',
  'com.apple.messages.effect.CKSpotlightEffect',
  'com.apple.messages.effect.CKHappyBirthdayEffect',
  'com.apple.messages.effect.CKConfettiEffect',
  'com.apple.messages.effect.CKHeartEffect',
  'com.apple.messages.effect.CKLasersEffect',
  'com.apple.messages.effect.CKFireworksEffect',
  'com.apple.messages.effect.CKShootingStarEffect',
  'com.apple.messages.effect.CKSparklesEffect',
]

const EDIT_WINDOW = 15 * 60_000

/** Field geometry, shared so the buttons beside it land on its centre line. */
const FIELD_HEIGHT = 34
const BUTTON_HIT = 28
const BUTTON_LIFT = (FIELD_HEIGHT - BUTTON_HIT) / 2

function Banner({ label, body, onClose, testId }: { label: string; body: string; onClose: () => void; testId: string }) {
  return (
    <div
      testId={testId}
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: S.x2,
        marginBottom: S.x2,
        paddingLeft: S.x2,
        paddingRight: S.x1,
        paddingTop: S.x1,
        paddingBottom: S.x1,
        borderRadius: RADIUS.control,
        backgroundColor: C.raised,
      }}
    >
      <div style={{ width: 2, height: 26, borderRadius: 1, backgroundColor: C.accent, flexShrink: 0 }} />
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0 }}>
        <text style={{ ...TYPE.micro, fontWeight: 600, color: C.accent, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{label}</text>
        <text style={{ ...TYPE.caption, color: C.secondary, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{body}</text>
      </div>
      <IconButton icon="close" label="Cancel" size={12} hit={24} onClick={onClose} />
    </div>
  )
}

export function Composer({ chat }: { chat: Chat }) {
  const shell = useShell()
  const { store } = shell
  const state = useAppState(store)
  const { renderer } = useGpuix()
  const draft = state.drafts[chat.guid] ?? ''
  const replyGuid = state.replyingTo[chat.guid]
  const editGuid = state.editing[chat.guid]
  const messages = state.messages[chat.guid] ?? []
  const replyTarget = replyGuid ? messages.find((item) => item.guid === replyGuid) : undefined
  const editTarget = editGuid ? messages.find((item) => item.guid === editGuid) : undefined
  const [effect, setEffect] = useState('none')
  const [attachOpen, setAttachOpen] = useState(false)
  const [attachPath, setAttachPath] = useState('')
  const textareaRef = useRef<PublicInstance | null>(null)
  const ready = draft.trim().length > 0
  const isSms = chat.service !== 'iMessage'
  const sendColor = isSms ? C.sms : C.accent

  useEffect(() => {
    if (textareaRef.current && renderer?.focusElement) renderer.focusElement(textareaRef.current.id)
  }, [chat.guid, renderer, replyGuid, editGuid])

  const send = (text: string) => {
    if (!text.trim()) return
    void store.send(chat.guid, text, { effect: effect === 'none' ? undefined : effect })
    setEffect('none')
  }

  const sendFile = () => {
    const path = attachPath.trim().replace(/^~(?=\/)/, process.env.HOME ?? '~')
    if (!path) return
    void store.sendAttachment(chat.guid, path)
    setAttachPath('')
    setAttachOpen(false)
  }

  const editLast = () => {
    const last = [...messages]
      .reverse()
      .find((item) => item.fromMe && !item.error && !item.dateRetracted && item.attachments.length === 0 && Date.now() - item.date < EDIT_WINDOW)
    if (last) store.setEditing(chat.guid, last.guid)
  }

  return (
    <div
      testId="composer"
      style={{ display: 'flex', flexDirection: 'column', flexShrink: 0, paddingLeft: S.x3, paddingRight: S.x3, paddingTop: S.x2, paddingBottom: S.x3, userSelect: 'none' }}
    >
      {replyTarget ? (
        <Banner
          testId="reply-banner"
          label={`Replying to ${replyTarget.fromMe ? 'yourself' : replyTarget.sender ? handleName(replyTarget.sender) : 'message'}`}
          body={replyTarget.text || 'Attachment'}
          onClose={() => store.setReplyingTo(chat.guid, undefined)}
        />
      ) : null}
      {editTarget ? <Banner testId="edit-banner" label="Editing" body={editTarget.text} onClose={() => store.setEditing(chat.guid, undefined)} /> : null}
      {attachOpen ? (
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x2, marginBottom: S.x2 }}>
          <TextField testId="attach-path" value={attachPath} onChange={setAttachPath} onSubmit={sendFile} placeholder="Path to a file, for example ~/Pictures/photo.jpg" autoFocus />
          <IconButton icon="send" label="Send file" onClick={sendFile} color={C.accent} strong disabled={attachPath.trim().length === 0} />
          <IconButton icon="close" label="Cancel" onClick={() => setAttachOpen(false)} />
        </div>
      ) : null}

      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-end', gap: S.x1 }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: S.x1, paddingBottom: BUTTON_LIFT, flexShrink: 0 }}>
          <IconButton icon="plus" label="Attach a file" testId="attach" hit={BUTTON_HIT} size={17} active={attachOpen} onClick={() => setAttachOpen((open) => !open)} />
          {state.capabilities.effects && !isSms ? (
            <Select value={effect} onValueChange={setEffect}>
              <div style={{ position: 'relative' }}>
                <SelectTrigger
                  style={{
                    width: BUTTON_HIT,
                    height: BUTTON_HIT,
                    borderRadius: RADIUS.control,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    cursor: 'pointer',
                    backgroundColor: effect !== 'none' ? C.selectedSoft : undefined,
                    hover: { backgroundColor: effect !== 'none' ? C.selectedSoft : C.hoverWash },
                    active: { backgroundColor: C.pressWash },
                  }}
                >
                  <Icon name="effect" size={16} color={effect !== 'none' ? C.accent : C.secondary} />
                </SelectTrigger>
                <SelectContent
                  side="top"
                  sideOffset={S.x2}
                  style={{
                    backgroundColor: C.overlay,
                    borderWidth: 1,
                    borderColor: C.overlayBorder,
                    borderRadius: RADIUS.menu,
                    padding: S.x1,
                    minWidth: 180,
                    boxShadow: overlayShadow,
                  }}
                >
                  {['none', ...EFFECTS].map((id) => (
                    <SelectItem
                      key={id}
                      value={id}
                      style={(item) => ({
                        height: 28,
                        paddingLeft: S.x2,
                        paddingRight: S.x2,
                        borderRadius: RADIUS.menuItem,
                        display: 'flex',
                        alignItems: 'center',
                        cursor: 'pointer',
                        backgroundColor: item.highlighted ? C.accent : C.overlay,
                      })}
                    >
                      {(item) => (
                        <text style={{ ...TYPE.body, color: item.highlighted ? C.onAccent : C.text, whiteSpace: 'nowrap' }}>
                          {id === 'none' ? 'No effect' : effectName(id)}
                        </text>
                      )}
                    </SelectItem>
                  ))}
                </SelectContent>
              </div>
            </Select>
          ) : null}
        </div>

        <div
          style={{
            display: 'flex',
            flexDirection: 'row',
            alignItems: 'flex-end',
            gap: S.x1,
            flexGrow: 1,
            flexShrink: 1,
            flexBasis: 0,
            minWidth: 0,
            minHeight: FIELD_HEIGHT,
            borderRadius: FIELD_HEIGHT / 2,
            borderWidth: 1,
            borderColor: C.separator,
            backgroundColor: C.canvas,
            paddingLeft: S.x3,
            paddingRight: S.x1,
            paddingTop: S.x1,
            paddingBottom: S.x1,
          }}
        >
          <textarea
            ref={textareaRef}
            testId="draft"
            value={draft}
            placeholder={editGuid ? 'Edit message' : isSms ? 'Text message' : 'iMessage'}
            minRows={1}
            maxRows={8}
            autoFocus
            theme={{ caret: C.accent, textMuted: C.tertiary }}
            style={{ flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, ...TYPE.bubble, color: C.text, backgroundColor: C.transparent, borderWidth: 0, paddingTop: 2, paddingBottom: 2 }}
            onChange={(event) => store.setDraft(chat.guid, event.value ?? '')}
            onSubmit={(event) => send(event.value ?? draft)}
            onKeyDown={(event) => {
              if (event.key === 'escape') {
                if (replyGuid) store.setReplyingTo(chat.guid, undefined)
                if (editGuid) store.setEditing(chat.guid, undefined)
                return
              }
              if (event.key === 'up' && draft.length === 0 && state.capabilities.edit) editLast()
              if (event.key === 'v' && primaryModifier(event.modifiers)) {
                void clipboardImage().then((path) => {
                  if (path) void store.sendAttachment(chat.guid, path)
                })
              }
            }}
          />
          <div
            testId="send"
            onClick={() => send(draft)}
            style={{
              width: 24,
              height: 24,
              borderRadius: 12,
              flexShrink: 0,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              cursor: ready ? 'pointer' : 'default',
              backgroundColor: ready ? sendColor : C.ghost,
              opacity: ready ? 1 : 0.5,
              hover: ready ? { opacity: 0.88 } : undefined,
              active: ready ? { opacity: 0.7 } : undefined,
            }}
          >
            <Icon name={editGuid ? 'check' : 'send'} size={14} color={C.onAccent} strong />
          </div>
        </div>
      </div>
    </div>
  )
}
