<script setup lang="ts">
import { Button, Flex, Input, InputNumber, Space, SpaceAddon, SpaceCompact, Switch, Tag } from 'antdv-next'
import { computed, ref } from 'vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { useBongoConnection } from '@/composables/useBongoConnection'
import { useConnectionStore } from '@/stores/connection'

const connectionStore = useConnectionStore()
const { createRoom, disconnect, joinRoom, reconnect } = useBongoConnection()
const roomCodeInput = ref(connectionStore.roomCode)

const statusColor = computed(() => {
  switch (connectionStore.status) {
    case 'connected':
      return 'success'
    case 'connecting':
    case 'reconnecting':
      return 'processing'
    case 'error':
      return 'error'
    default:
      return 'default'
  }
})

const canJoin = computed(() => {
  return roomCodeInput.value.trim().length > 0
})

function handleJoinRoom() {
  joinRoom(roomCodeInput.value)
}
</script>

<template>
  <ProList title="Pairing">
    <ProListItem
      description="Use ws:// for local development and wss:// for a deployed relay."
      title="Relay URL"
    >
      <Input
        v-model:value="connectionStore.relayUrl"
        class="w-72"
        placeholder="ws://localhost:8787/ws"
      />
    </ProListItem>

    <ProListItem
      description="Create a short room code and share it with one other BongoCat."
      title="Create Room"
    >
      <Button
        type="primary"
        @click="createRoom"
      >
        Create
      </Button>
    </ProListItem>

    <ProListItem
      description="Enter the room code from another BongoCat."
      title="Join Room"
    >
      <SpaceCompact>
        <Input
          v-model:value="roomCodeInput"
          class="w-36 uppercase"
          placeholder="ABC123"
        />

        <Button
          :disabled="!canJoin"
          @click="handleJoinRoom"
        >
          Join
        </Button>
      </SpaceCompact>
    </ProListItem>

    <ProListItem
      :description="connectionStore.error || (connectionStore.remoteProfile ? `Peer ${connectionStore.remoteProfile.peerId}` : 'Waiting for a peer.')"
      title="Status"
    >
      <Flex
        align="center"
        gap="small"
      >
        <Tag :color="statusColor">
          {{ connectionStore.status }}
        </Tag>

        <Tag v-if="connectionStore.roomCode">
          {{ connectionStore.roomCode }}
        </Tag>
      </Flex>
    </ProListItem>

    <ProListItem title="Connection">
      <Space>
        <Button @click="reconnect">
          Reconnect
        </Button>

        <Button
          danger
          @click="disconnect"
        >
          Disconnect
        </Button>
      </Space>
    </ProListItem>
  </ProList>

  <ProList title="Remote Cat Window">
    <ProListItem
      description="Show or hide the other computer's BongoCat."
      title="Visible"
    >
      <Switch v-model:checked="connectionStore.remoteWindow.visible" />
    </ProListItem>

    <ProListItem
      description="Let clicks pass through the remote cat window."
      title="Pass Through"
    >
      <Switch v-model:checked="connectionStore.remoteWindow.passThrough" />
    </ProListItem>

    <ProListItem
      description="Keep the remote cat above other windows."
      title="Always on Top"
    >
      <Switch v-model:checked="connectionStore.remoteWindow.alwaysOnTop" />
    </ProListItem>

    <ProListItem title="Window Size">
      <SpaceCompact>
        <InputNumber
          v-model:value="connectionStore.remoteWindow.scale"
          class="w-20"
          :max="500"
          :min="1"
        />

        <SpaceAddon>%</SpaceAddon>
      </SpaceCompact>
    </ProListItem>

    <ProListItem title="Opacity">
      <SpaceCompact>
        <InputNumber
          v-model:value="connectionStore.remoteWindow.opacity"
          class="w-20"
          :max="100"
          :min="10"
        />

        <SpaceAddon>%</SpaceAddon>
      </SpaceCompact>
    </ProListItem>
  </ProList>
</template>
