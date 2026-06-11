import { nanoid } from 'nanoid'
import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'

import type { ConnectionStatus, PeerProfile } from '@/types/connection'

export interface RemoteWindowSettings {
  visible: boolean
  passThrough: boolean
  alwaysOnTop: boolean
  scale: number
  opacity: number
}

export const useConnectionStore = defineStore('connection', () => {
  const relayUrl = ref('ws://localhost:8787/ws')
  const roomCode = ref('')
  const peerId = ref(nanoid())
  const status = ref<ConnectionStatus>('disconnected')
  const error = ref('')
  const remoteProfile = ref<PeerProfile>()
  const remoteWindow = reactive<RemoteWindowSettings>({
    visible: false,
    passThrough: false,
    alwaysOnTop: true,
    scale: 100,
    opacity: 100,
  })

  return {
    relayUrl,
    roomCode,
    peerId,
    status,
    error,
    remoteProfile,
    remoteWindow,
  }
})
