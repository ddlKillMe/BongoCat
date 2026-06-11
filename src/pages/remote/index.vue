<script setup lang="ts">
import { convertFileSrc } from '@tauri-apps/api/core'
import { PhysicalSize } from '@tauri-apps/api/dpi'
import { sep } from '@tauri-apps/api/path'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { exists, readDir } from '@tauri-apps/plugin-fs'
import { useEventListener } from '@vueuse/core'
import { findKey, nth } from 'es-toolkit/compat'
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'

import type { ModelMode } from '@/stores/model'
import type { BongoActivity } from '@/types/connection'

import { useTauriListen } from '@/composables/useTauriListen'
import { LISTEN_KEY } from '@/constants'
import { hideWindow, setAlwaysOnTop, showWindow } from '@/plugins/window'
import { useConnectionStore } from '@/stores/connection'
import { useModelStore } from '@/stores/model'
import { isImage } from '@/utils/is'
import { Live2d } from '@/utils/live2d'
import { join } from '@/utils/path'
import { clearObject } from '@/utils/shared'

interface ModelSize {
  width: number
  height: number
}

interface StickState {
  x: number
  y: number
  moved: boolean
  pressed: boolean
}

const REMOTE_CANVAS_ID = 'remoteLive2dCanvas'
const appWindow = getCurrentWebviewWindow()
const connectionStore = useConnectionStore()
const modelStore = useModelStore()
const live2d = new Live2d(REMOTE_CANVAS_ID)
const modelSize = ref<ModelSize>()
const currentMode = ref<ModelMode>()
const backgroundImagePath = ref<string>()
const supportKeys = reactive<Record<string, string>>({})
const pressedKeys = reactive<Record<string, string>>({})
const sticks = reactive({
  left: { x: 0, y: 0, moved: false, pressed: false } satisfies StickState,
  right: { x: 0, y: 0, moved: false, pressed: false } satisfies StickState,
})

const currentModel = computed(() => {
  return modelStore.models.find(item => item.isPreset && item.mode === (currentMode.value ?? 'standard'))
    ?? modelStore.models.find(item => item.isPreset && item.mode === 'standard')
})

onMounted(async () => {
  await loadMode(connectionStore.remoteProfile?.modelMode ?? 'standard')
})

onUnmounted(() => {
  live2d.destroy()
})

useEventListener('resize', () => {
  resizeModel()
})

watch(() => connectionStore.remoteProfile?.modelMode, async (mode) => {
  if (!mode) return

  await loadMode(mode)
})

watch(() => connectionStore.remoteWindow.visible, (value) => {
  value ? showWindow() : hideWindow()
}, { immediate: true })

watch(() => connectionStore.remoteWindow.passThrough, (value) => {
  appWindow.setIgnoreCursorEvents(value)
}, { immediate: true })

watch(() => connectionStore.remoteWindow.alwaysOnTop, setAlwaysOnTop, { immediate: true })

watch([() => connectionStore.remoteWindow.scale, modelSize], ([scale, modelSize]) => {
  if (!modelSize) return

  appWindow.setSize(
    new PhysicalSize({
      width: Math.round(modelSize.width * (scale / 100)),
      height: Math.round(modelSize.height * (scale / 100)),
    }),
  )
}, { immediate: true })

watch(sticks.left, ({ x, y, pressed }) => {
  sticks.left.moved = x !== 0 || y !== 0

  live2d.setParameterValue('CatParamStickShowLeftHand', sticks.left.moved || pressed)
}, { deep: true })

watch(sticks.right, ({ x, y, pressed }) => {
  sticks.right.moved = x !== 0 || y !== 0

  live2d.setParameterValue('CatParamStickShowRightHand', sticks.right.moved || pressed)
}, { deep: true })

useTauriListen<BongoActivity>(LISTEN_KEY.REMOTE_ACTIVITY, ({ payload }) => {
  void applyActivity(payload)
})

async function loadMode(mode: ModelMode) {
  if (currentMode.value === mode && live2d.model) return

  currentMode.value = mode
  clearObject([supportKeys, pressedKeys])

  const model = currentModel.value

  if (!model) return

  const { width, height } = await live2d.load(model.path)

  modelSize.value = { width, height }
  resizeModel()

  const backgroundPath = join(model.path, 'resources', 'background.png')
  const backgroundExisted = await exists(backgroundPath)

  backgroundImagePath.value = backgroundExisted ? convertFileSrc(backgroundPath) : void 0

  const resourcePath = join(model.path, 'resources')

  for (const groupName of ['left-keys', 'right-keys']) {
    const groupDir = join(resourcePath, groupName)
    const files = await readDir(groupDir).catch(() => [])
    const imageFiles = files.filter(file => isImage(file.name))

    for (const file of imageFiles) {
      const fileName = file.name.split('.')[0]

      supportKeys[fileName] = join(groupDir, file.name)
    }
  }
}

function resizeModel() {
  if (!modelSize.value) return

  live2d.resizeModel(modelSize.value)
}

function getSupportedKey(key: string) {
  let nextKey = key
  const unsupportedKey = !supportKeys[nextKey]

  if (key.startsWith('F') && unsupportedKey) {
    nextKey = key.replace(/F(\d+)/, 'Fn')
  }

  for (const item of ['Meta', 'Shift', 'Alt', 'Control']) {
    if (key.startsWith(item) && unsupportedKey) {
      const regex = new RegExp(`^(${item}).*`)
      nextKey = key.replace(regex, '$1')
    }
  }

  return nextKey
}

function handlePress(key: string) {
  const path = supportKeys[getSupportedKey(key)]

  if (!path) return

  const dirName = nth(path.split(sep()), -2)!
  const prevKey = findKey(pressedKeys, (value) => {
    return value.includes(dirName)
  })

  if (prevKey) {
    handleRelease(prevKey)
  }

  pressedKeys[key] = path
  updateHands()
}

function handleRelease(key: string) {
  delete pressedKeys[key]
  delete pressedKeys[getSupportedKey(key)]
  updateHands()
}

function updateHands() {
  const dirs = Object.values(pressedKeys).map((path) => {
    return nth(path.split(sep()), -2)!
  })

  const hasLeft = dirs.some(dir => dir.startsWith('left'))
  const hasRight = dirs.some(dir => dir.startsWith('right'))

  live2d.setParameterValue('CatParamLeftHandDown', sticks.left.moved || sticks.left.pressed || hasLeft)
  live2d.setParameterValue('CatParamRightHandDown', sticks.right.moved || sticks.right.pressed || hasRight)
}

function handleMouseChange(key: string, pressed = true) {
  const id = key === 'Left' ? 'ParamMouseLeftDown' : 'ParamMouseRightDown'

  live2d.setParameterValue(id, pressed)
}

async function handleAxisChange(id: string, value: number) {
  const range = live2d.getParameterValueRange(id)

  if (!range) return

  const { min, max } = range

  live2d.setParameterValue(id, Math.max(min, value * max))
}

async function applyActivity(activity: BongoActivity) {
  await loadMode(activity.modelMode)

  switch (activity.kind) {
    case 'ModelChanged':
      return
    case 'KeyboardPress':
      return handlePress(activity.value)
    case 'KeyboardRelease':
      return handleRelease(activity.value)
    case 'MousePress':
      return handleMouseChange(activity.value)
    case 'MouseRelease':
      return handleMouseChange(activity.value, false)
    case 'MouseMove':
      return live2d.setMouseRatio(activity.value.xRatio, activity.value.yRatio)
    case 'GamepadAxisChanged':
      return handleGamepadAxis(activity.name, activity.value)
    case 'GamepadButtonChanged':
      return handleGamepadButton(activity.name, activity.value)
  }
}

function handleGamepadAxis(name: string, value: number) {
  switch (name) {
    case 'LeftStickX':
      sticks.left.x = value

      return handleAxisChange('CatParamStickLX', value)
    case 'LeftStickY':
      sticks.left.y = value

      return handleAxisChange('CatParamStickLY', value)
    case 'RightStickX':
      sticks.right.x = value

      return handleAxisChange('CatParamStickRX', value)
    case 'RightStickY':
      sticks.right.y = value

      return handleAxisChange('CatParamStickRY', value)
  }
}

function handleGamepadButton(name: string, value: number) {
  switch (name) {
    case 'LeftThumb':
      sticks.left.pressed = value !== 0

      live2d.setParameterValue('CatParamStickLeftDown', value !== 0)
      updateHands()

      return
    case 'RightThumb':
      sticks.right.pressed = value !== 0

      live2d.setParameterValue('CatParamStickRightDown', value !== 0)
      updateHands()

      return
    default:
      return value > 0 ? handlePress(name) : handleRelease(name)
  }
}

function handleMouseDown() {
  appWindow.startDragging()
}
</script>

<template>
  <div
    class="relative size-screen overflow-hidden children:(absolute size-full)"
    :style="{
      opacity: connectionStore.remoteWindow.opacity / 100,
    }"
    @mousedown="handleMouseDown"
  >
    <img
      v-if="backgroundImagePath"
      class="object-cover"
      :src="backgroundImagePath"
    >

    <canvas :id="REMOTE_CANVAS_ID" />

    <img
      v-for="path in pressedKeys"
      :key="path"
      class="object-cover"
      :src="convertFileSrc(path)"
    >
  </div>
</template>
