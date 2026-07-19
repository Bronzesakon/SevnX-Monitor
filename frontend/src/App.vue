<script setup lang="ts">
import { computed, markRaw, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch, type Component } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import IconButton from './components/IconButton.vue'
import BarView from './views/BarView.vue'
import { hideMainWindow, isTauriRuntime, setMainWindowHeight } from './lib/ipc'
import { lastUpdated } from './lib/format'
import { useAppStore } from './stores/app'
import type { RefreshStatus } from './types/contracts'
import DashboardView from './views/DashboardView.vue'
import LoginGuide from './views/LoginGuide.vue'
import SettingsView from './views/SettingsView.vue'

const app = useAppStore()
const shell = ref<HTMLElement | null>(null)
const usageViewComponent = shallowRef<Component | null>(null)
let usageViewPromise: Promise<void> | undefined
const isBarWindow = isTauriRuntime() && getCurrentWindow().label === 'bar'
const showLogin = computed(() => (
  app.initialized
  && app.auth !== 'authenticated'
  && app.auth !== 'validating'
))

if (isBarWindow && typeof document !== 'undefined') {
  document.documentElement.classList.add('sevnx-window-bar')
}
let unlistenNavigateSettings: UnlistenFn | undefined
let contentResizeObserver: ResizeObserver | undefined
let resizeScheduled = false
const loginMessage = computed(() => {
  if (app.actionError) return app.actionError
  if (app.auth === 'expired' || app.auth === 'loggedOut') {
    return app.snapshot?.lastError?.message ?? (app.auth === 'expired' ? '登录状态已失效，请重新登录。' : null)
  }
  return null
})

function applyTheme(): void {
  document.documentElement.dataset.theme = app.settings.theme
}

function preloadUsageView(): Promise<void> {
  usageViewPromise ??= import('./views/UsageView.vue')
    .then((module) => {
      usageViewComponent.value = markRaw(module.default)
    })
    .catch((error: unknown) => {
      usageViewPromise = undefined
      throw error
    })
  return usageViewPromise
}

async function showUsage(): Promise<void> {
  try {
    await preloadUsageView()
    app.activeView = 'usage'
  } catch {
    app.actionError = '使用记录界面加载失败，请重试。'
  }
}

const displayRefresh = computed<RefreshStatus>(() => {
  if (app.refresh === 'failed' || app.refresh === 'stale') return app.refresh
  if (!app.initialized || app.auth === 'validating') return 'refreshing'
  return app.refresh
})

const refreshLabel = computed(() => {
  if (displayRefresh.value === 'refreshing') return '正在更新'
  if (displayRefresh.value === 'failed') return '刷新失败'
  if (displayRefresh.value === 'stale') return '数据待更新'
  return app.snapshot?.lastSuccessAt ? `刷新 ${lastUpdated(app.snapshot.lastSuccessAt)}` : '等待刷新'
})

async function hideWindow(): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    await hideMainWindow()
  } catch {}
}

function scheduleWindowSize(): void {
  if (!isTauriRuntime() || isBarWindow || resizeScheduled) return
  resizeScheduled = true
  requestAnimationFrame(async () => {
    resizeScheduled = false
    await nextTick()
    const header = shell.value?.querySelector<HTMLElement>('.app-header, .login-window-chrome')
    const error = shell.value?.querySelector<HTMLElement>('.action-error')
    const section = shell.value?.querySelector<HTMLElement>('.section-surface')
    if (!section) return
    const height = (header?.offsetHeight ?? 0) + (error?.offsetHeight ?? 0) + section.scrollHeight + 2
    try {
      await setMainWindowHeight(height)
    } catch {}
  })
}

onMounted(async () => {
  if (!isBarWindow) void preloadUsageView().catch(() => undefined)
  await app.initialize()
  applyTheme()
  if (!isBarWindow && shell.value) {
    contentResizeObserver = new ResizeObserver(scheduleWindowSize)
    contentResizeObserver.observe(shell.value)
    scheduleWindowSize()
  }
  if (isTauriRuntime() && !isBarWindow) {
    unlistenNavigateSettings = await listen('navigate-settings', () => {
      app.activeView = 'settings'
    })
  }
})

watch(() => app.settings.theme, applyTheme)
watch([() => app.activeView, () => app.usagePanel, () => app.snapshot, () => app.initialized], scheduleWindowSize, { deep: true })

onBeforeUnmount(() => {
  unlistenNavigateSettings?.()
  contentResizeObserver?.disconnect()
  app.dispose()
})
</script>

<template>
  <div ref="shell" class="app-shell" :class="{ 'app-shell--bar': isBarWindow }">
    <BarView v-if="isBarWindow" :dashboard="app.snapshot?.dashboard" />
    <template v-else-if="showLogin">
      <header class="login-window-chrome">
        <div class="window-drag-space" data-tauri-drag-region aria-hidden="true"></div>
        <IconButton icon="close" label="隐藏到托盘" @click="hideWindow" />
      </header>
      <LoginGuide :busy="app.pending || app.auth === 'loggingIn' || app.auth === 'validating'" :message="loginMessage" @login="app.login" />
    </template>
    <template v-else>
      <header class="app-header" data-tauri-drag-region>
        <nav class="top-tabs" aria-label="主页面">
          <button class="top-tab" :class="{ 'is-active': app.activeView === 'dashboard' }" type="button" @click="app.activeView = 'dashboard'">仪表盘</button>
          <button class="top-tab" :class="{ 'is-active': app.activeView === 'usage' }" type="button" @click="showUsage">使用记录</button>
        </nav>
        <div class="window-drag-space" aria-hidden="true"></div>
        <div class="window-actions">
          <div class="refresh-status" :class="`refresh-status--${displayRefresh}`" role="status" aria-live="polite">
            <span class="refresh-status__dot" />
            <span>{{ refreshLabel }}</span>
          </div>
          <IconButton icon="refresh" label="手动刷新" :spinning="displayRefresh === 'refreshing'" :disabled="app.pending || displayRefresh === 'refreshing'" @click="app.runRefresh" />
          <IconButton icon="settings" label="设置" :active="app.activeView === 'settings'" @click="app.activeView = 'settings'" />
          <IconButton icon="close" label="隐藏到托盘" @click="hideWindow" />
        </div>
      </header>
      <p v-if="app.actionError" class="action-error" role="alert">{{ app.actionError }}</p>
      <DashboardView v-if="app.activeView === 'dashboard'" :dashboard="app.snapshot?.dashboard" />
      <component
        :is="usageViewComponent"
        v-else-if="app.activeView === 'usage' && usageViewComponent"
        :usage="app.snapshot?.usage"
      />
      <SettingsView v-else />
    </template>
    <Transition name="toast">
      <div v-if="app.toast" class="toast" role="status">{{ app.toast }}</div>
    </Transition>
  </div>
</template>
