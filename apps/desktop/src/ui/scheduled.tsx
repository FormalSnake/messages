import { formatScheduledFor, type ScheduledMessage } from '@messages/core'
import { useAppState } from './use-app-state'
import { useShell } from './context'
import { Icon } from './icons'
import { IconButton } from './primitives'
import { C, RADIUS, S, TYPE } from './theme'

function ScheduledRow({ item }: { item: ScheduledMessage }) {
  const { store } = useShell()
  return (
    <div
      testId="scheduled-row"
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
      <Icon name="schedule" size={14} color={C.accent} />
      <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0 }}>
        <text style={{ ...TYPE.caption, color: C.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{item.text}</text>
        <text style={{ ...TYPE.micro, color: C.secondary, whiteSpace: 'nowrap' }}>{`Sends at ${formatScheduledFor(item.sendAt)}`}</text>
      </div>
      <IconButton icon="close" label="Cancel send" size={12} hit={24} onClick={() => void store.cancelScheduled(item.id)} />
    </div>
  )
}

export function ScheduledMessages({ chatGuid }: { chatGuid: string }) {
  const { store } = useShell()
  const state = useAppState(store)
  const items = state.scheduled.filter((item) => item.chatGuid === chatGuid)
  if (items.length === 0) return null
  return (
    <div testId="scheduled-list" style={{ display: 'flex', flexDirection: 'column' }}>
      {items.map((item) => (
        <ScheduledRow key={item.id} item={item} />
      ))}
    </div>
  )
}
