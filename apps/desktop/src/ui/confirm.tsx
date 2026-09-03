import { useWindowSize } from '@gpuix/react'
import { C, RADIUS, S, TYPE } from './theme'
import { Button, overlayShadow } from './primitives'

export interface ConfirmRequest {
  title: string
  body?: string
  /** Label of the button that goes through with it. */
  action: string
  danger?: boolean
  onConfirm: () => void
}

/** One question, two buttons. Enter confirms, Escape or a click outside cancels. */
export function ConfirmDialog({ request, onClose }: { request: ConfirmRequest; onClose: () => void }) {
  const { width, height } = useWindowSize()
  const confirm = () => {
    onClose()
    request.onConfirm()
  }
  return (
    <anchored deferred occlude priority={4} position={{ x: 0, y: 0 }}>
      <div
        testId="confirm"
        autoFocus
        tabIndex={-1}
        onKeyDown={(event) => {
          if (event.key === 'escape') onClose()
          else if (event.key === 'enter') confirm()
        }}
        style={{ width, height, backgroundColor: '#00000080', pointerEvents: 'auto', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
      >
        <div
          onMouseDownOutside={onClose}
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: S.x2,
            width: 340,
            padding: S.x5,
            borderRadius: RADIUS.card,
            backgroundColor: C.overlay,
            borderWidth: 1,
            borderColor: C.overlayBorder,
            boxShadow: overlayShadow,
            userSelect: 'none',
          }}
        >
          <text style={{ ...TYPE.title, color: C.text }}>{request.title}</text>
          {request.body ? <text style={{ ...TYPE.caption, color: C.secondary }}>{request.body}</text> : null}
          <div style={{ display: 'flex', flexDirection: 'row', justifyContent: 'flex-end', gap: S.x2, paddingTop: S.x2 }}>
            <Button testId="confirm-cancel" onClick={onClose}>
              Cancel
            </Button>
            <Button testId="confirm-action" kind={request.danger ? 'danger' : 'primary'} onClick={confirm}>
              {request.action}
            </Button>
          </div>
        </div>
      </div>
    </anchored>
  )
}
