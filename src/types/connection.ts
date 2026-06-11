import type { ModelMode } from '@/stores/model'

export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'error'

export interface PeerProfile {
  peerId: string
  modelMode: ModelMode
}

interface ActivityBase {
  modelMode: ModelMode
}

export type BongoActivity
  = | (ActivityBase & {
    kind: 'ModelChanged'
  })
  | (ActivityBase & {
    kind: 'KeyboardPress' | 'KeyboardRelease' | 'MousePress' | 'MouseRelease'
    value: string
  })
  | (ActivityBase & {
    kind: 'MouseMove'
    value: {
      xRatio: number
      yRatio: number
    }
  })
  | (ActivityBase & {
    kind: 'GamepadButtonChanged' | 'GamepadAxisChanged'
    name: string
    value: number
  })

export type ClientRelayMessage
  = | {
    type: 'create_room'
    profile: PeerProfile
  }
  | {
    type: 'join_room'
    roomCode: string
    profile: PeerProfile
  }
  | {
    type: 'activity'
    activity: BongoActivity
  }
  | {
    type: 'ping'
  }

export type ServerRelayMessage
  = | {
    type: 'room_created' | 'room_joined'
    roomCode: string
  }
  | {
    type: 'peer_joined'
    profile: PeerProfile
  }
  | {
    type: 'peer_left'
    peerId: string
  }
  | {
    type: 'activity'
    peerId: string
    activity: BongoActivity
  }
  | {
    type: 'heartbeat'
  }
  | {
    type: 'error'
    code: string
    message: string
  }
