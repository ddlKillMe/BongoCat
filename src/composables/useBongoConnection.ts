import { emit, listen } from '@tauri-apps/api/event'
import { message } from 'antdv-next'
import { watch } from 'vue'

import type { BongoActivity, ClientRelayMessage, ServerRelayMessage } from '@/types/connection'

import { LISTEN_KEY, WINDOW_LABEL } from '@/constants'
import { showWindow } from '@/plugins/window'
import { useConnectionStore } from '@/stores/connection'
import { useModelStore } from '@/stores/model'

let socket: WebSocket | undefined
let reconnectTimer: ReturnType<typeof setTimeout> | undefined
let reconnectAttempt = 0
let manuallyClosed = false
let listenersStarted = false
let modelWatcherStarted = false

export function useBongoConnection() {
  const connectionStore = useConnectionStore()
  const modelStore = useModelStore()

  function getProfile() {
    return {
      peerId: connectionStore.peerId,
      modelMode: modelStore.currentModel?.mode ?? 'standard',
    }
  }

  function send(payload: ClientRelayMessage) {
    if (socket?.readyState !== WebSocket.OPEN) return false

    socket.send(JSON.stringify(payload))

    return true
  }

  function clearReconnectTimer() {
    if (!reconnectTimer) return

    clearTimeout(reconnectTimer)

    reconnectTimer = void 0
  }

  function disconnect() {
    manuallyClosed = true
    clearReconnectTimer()
    socket?.close()
    socket = void 0
    connectionStore.status = 'disconnected'
    connectionStore.error = ''
    connectionStore.remoteProfile = void 0
  }

  function scheduleReconnect() {
    if (manuallyClosed || !connectionStore.roomCode) return

    connectionStore.status = 'reconnecting'
    clearReconnectTimer()

    const delay = Math.min(30000, 1000 * 2 ** reconnectAttempt)
    reconnectAttempt += 1

    reconnectTimer = setTimeout(() => {
      connect({
        type: 'join_room',
        roomCode: connectionStore.roomCode,
        profile: getProfile(),
      }, true)
    }, delay)
  }

  function connect(initialMessage: ClientRelayMessage, reconnecting = false) {
    manuallyClosed = false
    clearReconnectTimer()
    socket?.close()

    connectionStore.status = reconnecting ? 'reconnecting' : 'connecting'
    connectionStore.error = ''

    try {
      socket = new WebSocket(connectionStore.relayUrl)
    } catch (error) {
      connectionStore.error = String(error)
      connectionStore.status = 'error'

      return
    }

    socket.addEventListener('open', () => {
      reconnectAttempt = 0
      send(initialMessage)
    })

    socket.addEventListener('message', ({ data }) => {
      handleMessage(data)
    })

    socket.addEventListener('close', () => {
      socket = void 0

      if (manuallyClosed) return

      scheduleReconnect()
    })

    socket.addEventListener('error', () => {
      connectionStore.error = 'Connection failed.'
      connectionStore.status = 'error'
    })
  }

  function createRoom() {
    connectionStore.roomCode = ''
    connectionStore.remoteProfile = void 0
    connect({
      type: 'create_room',
      profile: getProfile(),
    })
  }

  function joinRoom(roomCode = connectionStore.roomCode) {
    connectionStore.roomCode = roomCode.trim().toUpperCase()
    connectionStore.remoteProfile = void 0

    connect({
      type: 'join_room',
      roomCode: connectionStore.roomCode,
      profile: getProfile(),
    })
  }

  function reconnect() {
    if (!connectionStore.roomCode) return

    connect({
      type: 'join_room',
      roomCode: connectionStore.roomCode,
      profile: getProfile(),
    }, true)
  }

  function handleMessage(data: unknown) {
    let payload: ServerRelayMessage

    try {
      payload = JSON.parse(String(data)) as ServerRelayMessage
    } catch {
      connectionStore.error = 'Relay sent an invalid message.'
      connectionStore.status = 'error'

      return
    }

    switch (payload.type) {
      case 'room_created':
      case 'room_joined':
        connectionStore.roomCode = payload.roomCode
        connectionStore.status = 'connected'
        connectionStore.error = ''
        return
      case 'peer_joined':
        connectionStore.remoteProfile = payload.profile
        connectionStore.remoteWindow.visible = true
        connectionStore.status = 'connected'
        showWindow(WINDOW_LABEL.REMOTE)
        return
      case 'peer_left':
        connectionStore.remoteProfile = void 0
        connectionStore.status = 'connected'
        return
      case 'activity':
        connectionStore.remoteWindow.visible = true
        showWindow(WINDOW_LABEL.REMOTE)
        emit(LISTEN_KEY.REMOTE_ACTIVITY, payload.activity)
        return
      case 'heartbeat':
        return
      case 'error':
        connectionStore.error = payload.message
        connectionStore.status = 'error'
        message.error(payload.message)
    }
  }

  function startListeners() {
    if (listenersStarted) return

    listenersStarted = true

    void listen<BongoActivity>(LISTEN_KEY.LOCAL_ACTIVITY, ({ payload }) => {
      if (connectionStore.status !== 'connected') return

      send({
        type: 'activity',
        activity: {
          ...payload,
          modelMode: modelStore.currentModel?.mode ?? payload.modelMode,
        },
      })
    })
  }

  function startModelWatcher() {
    if (modelWatcherStarted) return

    modelWatcherStarted = true

    watch(() => modelStore.currentModel?.mode, (modelMode) => {
      if (!modelMode || connectionStore.status !== 'connected') return

      send({
        type: 'activity',
        activity: {
          kind: 'ModelChanged',
          modelMode,
        },
      })
    })
  }

  startListeners()
  startModelWatcher()

  return {
    createRoom,
    disconnect,
    joinRoom,
    reconnect,
  }
}
