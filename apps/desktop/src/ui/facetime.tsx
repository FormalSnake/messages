import { openExternal } from '@messages/core'
import { C, TYPE } from './theme'
import { Icon } from './icons'
import { Button, IconButton } from './primitives'
import { useShell } from './context'
import { useAppState } from './use-app-state'

/** Incoming call banner. Answering asks the Mac to pick up and hand us a FaceTime Link for the browser. */
export function FaceTimeBanner({ offsetRight = 12 }: { offsetRight?: number }) {
  const shell = useShell()
  const state = useAppState(shell.store)
  const call = state.facetime
  if (!call) return null
  const who = call.from ?? 'Unknown caller'
  const line =
    call.status === 'incoming'
      ? call.canAnswer
        ? 'Incoming FaceTime'
        : 'Incoming FaceTime, answer on your Mac'
      : call.status === 'answering'
        ? 'Answering on the Mac and creating a link…'
        : call.status === 'ready'
          ? 'Link ready. The Mac leaves the call about 15 seconds after you join.'
          : call.status === 'ended'
            ? 'Call ended'
            : (call.error ?? 'Could not answer')
  return (
    <div
      testId="facetime-banner"
      style={{
        position: 'absolute',
        top: 12,
        right: offsetRight,
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: 12,
        paddingLeft: 14,
        paddingRight: 8,
        paddingTop: 10,
        paddingBottom: 10,
        borderRadius: 12,
        backgroundColor: C.overlay,
        borderWidth: 1,
        borderColor: C.overlayBorder,
        pointerEvents: 'auto',
        boxShadow: { offsetX: 0, offsetY: 8, blurRadius: 24, spreadRadius: 0, color: '#00000080' },
        userSelect: 'none',
      }}
    >
      <Icon name="video" size={20} color={call.status === 'failed' ? C.danger : C.online} />
      <div style={{ display: 'flex', flexDirection: 'column', gap: 1, maxWidth: 320 }}>
        <text style={{ ...TYPE.body, fontWeight: 600, color: C.text }}>{who}</text>
        <text style={{ ...TYPE.caption, color: C.secondary }}>{line}</text>
      </div>
      {call.status === 'incoming' && call.canAnswer ? (
        <Button kind="primary" testId="facetime-answer" onClick={() => void shell.store.answerFaceTime()}>
          Answer in browser
        </Button>
      ) : null}
      {call.status === 'ready' && call.link ? (
        <Button kind="primary" testId="facetime-join" onClick={() => openExternal(call.link!)}>
          Join
        </Button>
      ) : null}
      {call.status === 'incoming' && call.canAnswer ? (
        <Button kind="danger" testId="facetime-decline" onClick={() => void shell.store.declineFaceTime()}>
          Decline
        </Button>
      ) : null}
      <IconButton icon="close" label="Dismiss" onClick={() => shell.store.dismissFaceTime()} />
    </div>
  )
}
