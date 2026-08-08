import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import {
  getAppSnapshot,
  getSettings,
  listenForAuthStatus,
  listenForRefreshStatus,
  listenForSettings,
  listenForSnapshot,
  openDashboardInBrowser,
  openLoginWindow,
  openLogFile as openLogFileIpc,
  refreshAll,
  setBarVisible,
  setUsageRange,
  testRefresh,
  updateSettings,
} from '../lib/ipc'
import type {
  AppSettings,
  AppSnapshot,
  AuthStatus,
  RefreshStatus,
  UsageRange,
} from '../types/contracts'

export type MainView = 'dashboard' | 'usage' | 'settings'
export type UsagePanel = 'trend' | 'models' | 'groups' | 'endpoints'

const defaultSettings: AppSettings = {
  accessUrl: 'https://www.sevnx.lol',
  theme: 'system',
  autoRefresh: true,
  lowBalanceAlert: false,
  lowBalanceThreshold: 5,
  alertInterval: '15m',
  barVisible: true,
  autostart: false,
}

function publicMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message
  return '暂时无法完成操作，请稍后重试。'
}

export const useAppStore = defineStore('app', () => {
  const snapshot = ref<AppSnapshot | null>(null)
  const settings = ref<AppSettings>({ ...defaultSettings })
  const activeView = ref<MainView>('dashboard')
  const usagePanel = ref<UsagePanel>('trend')
  const pending = ref(false)
  const initialized = ref(false)
  const actionError = ref<string | null>(null)
  const toast = ref<string | null>(null)
  const unlisteners: UnlistenFn[] = []

  const auth = computed<AuthStatus>(() => snapshot.value?.auth ?? 'loggedOut')
  const refresh = computed<RefreshStatus>(() => snapshot.value?.refresh ?? 'idle')
  const usageRange = computed<UsageRange>(() => snapshot.value?.usage?.range ?? 'last24Hours')

  function replaceSnapshot(next: AppSnapshot): void {
    snapshot.value = next
  }

  async function initialize(): Promise<void> {
    if (initialized.value) return
    pending.value = true
    actionError.value = null
    try {
      let receivedSnapshotEvent = false
      let receivedSettingsEvent = false
      const subscriptions = await Promise.all([
        listenForSnapshot((nextSnapshot) => {
          receivedSnapshotEvent = true
          replaceSnapshot(nextSnapshot)
        }),
        listenForRefreshStatus((event) => {
          if (!snapshot.value) return
          snapshot.value = { ...snapshot.value, ...event }
        }),
        listenForAuthStatus((authStatus) => {
          if (!snapshot.value) return
          snapshot.value = { ...snapshot.value, auth: authStatus }
        }),
        listenForSettings((nextSettings) => {
          receivedSettingsEvent = true
          settings.value = nextSettings
        }),
      ])
      unlisteners.push(...subscriptions.filter((unlisten): unlisten is UnlistenFn => !!unlisten))
      const [nextSnapshot, nextSettings] = await Promise.all([getAppSnapshot(), getSettings()])
      if (!receivedSnapshotEvent) replaceSnapshot(nextSnapshot)
      if (!receivedSettingsEvent) settings.value = nextSettings
      initialized.value = true
    } catch (error) {
      actionError.value = publicMessage(error)
    } finally {
      pending.value = false
    }
  }

  async function runRefresh(): Promise<void> {
    if (pending.value || refresh.value === 'refreshing' || auth.value === 'validating') return
    pending.value = true
    actionError.value = null
    try {
      replaceSnapshot(await refreshAll())
    } catch (error) {
      actionError.value = publicMessage(error)
    } finally {
      pending.value = false
    }
  }

  async function selectUsageRange(range: UsageRange): Promise<void> {
    if (
      range === usageRange.value
      || pending.value
      || refresh.value === 'refreshing'
      || auth.value === 'validating'
    ) return
    pending.value = true
    actionError.value = null
    try {
      replaceSnapshot(await setUsageRange(range))
    } catch (error) {
      actionError.value = publicMessage(error)
    } finally {
      pending.value = false
    }
  }

  async function saveSettings(patch: Partial<AppSettings>): Promise<void> {
    const previous = settings.value
    const next = { ...settings.value, ...patch }
    settings.value = next
    actionError.value = null
    try {
      const saved = await updateSettings(next)
      settings.value = saved
      if (patch.barVisible !== undefined) await setBarVisible(patch.barVisible)
    } catch (error) {
      settings.value = previous
      actionError.value = publicMessage(error)
    }
  }

  async function login(): Promise<void> {
    actionError.value = null
    try {
      await openLoginWindow()
    } catch (error) {
      actionError.value = publicMessage(error)
    }
  }

  async function testRefreshToken(): Promise<void> {
    actionError.value = null
    try {
      await testRefresh()
      toast.value = '刷新请求已执行，结果见日志'
      window.setTimeout(() => {
        toast.value = null
      }, 2200)
    } catch (error) {
      actionError.value = publicMessage(error)
    }
  }

  async function openOfficialDashboard(): Promise<void> {
    actionError.value = null
    try {
      await openDashboardInBrowser()
    } catch (error) {
      actionError.value = publicMessage(error)
    }
  }

  async function openLogFile(): Promise<void> {
    actionError.value = null
    try {
      await openLogFileIpc()
    } catch (error) {
      actionError.value = publicMessage(error)
    }
  }

  function dispose(): void {
    unlisteners.splice(0).forEach((unlisten) => unlisten())
  }

  return {
    snapshot,
    settings,
    activeView,
    usagePanel,
    pending,
    initialized,
    actionError,
    toast,
    auth,
    refresh,
    usageRange,
    initialize,
    runRefresh,
    selectUsageRange,
    saveSettings,
    login,
    testRefreshToken,
    openOfficialDashboard,
    openLogFile,
    dispose,
  }
})
