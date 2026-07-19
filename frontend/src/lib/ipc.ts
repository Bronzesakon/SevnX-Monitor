import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AppSettings,
  AppSnapshot,
  AuthStatus,
  RefreshStatusEvent,
  UsageRange,
} from '../types/contracts'

type SnapshotEvent = AppSnapshot | { snapshot: AppSnapshot }
type BrowserDevelopmentFallback = {
  settings: AppSettings
  snapshot: AppSnapshot
}

let browserDevelopmentFallback: BrowserDevelopmentFallback | null = null

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export function installBrowserDevelopmentFallback(fallback: BrowserDevelopmentFallback): void {
  browserDevelopmentFallback = fallback
}

function useBrowserDevelopmentFallback<T>(
  select: (fallback: BrowserDevelopmentFallback) => T,
  error: unknown,
): T {
  if (import.meta.env.DEV && !isTauriRuntime() && browserDevelopmentFallback) {
    return structuredClone(select(browserDevelopmentFallback))
  }
  throw error
}

export async function getAppSnapshot(): Promise<AppSnapshot> {
  try {
    return await invoke<AppSnapshot>('get_app_snapshot')
  } catch (error) {
    return useBrowserDevelopmentFallback(({ snapshot }) => snapshot, error)
  }
}

export async function refreshAll(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('refresh_all')
}

export async function openLoginWindow(): Promise<void> {
  await invoke('open_login_window')
}

export async function openDashboardInBrowser(): Promise<void> {
  await invoke('open_dashboard_in_browser')
}

export async function setUsageRange(range: UsageRange): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_usage_range', { range })
}

export async function getSettings(): Promise<AppSettings> {
  try {
    return await invoke<AppSettings>('get_settings')
  } catch (error) {
    return useBrowserDevelopmentFallback(({ settings }) => settings, error)
  }
}

export async function updateSettings(settings: AppSettings): Promise<AppSettings> {
  return invoke<AppSettings>('update_settings', { settings })
}

export async function showMainWindow(): Promise<void> {
  await invoke('show_main_window')
}

export async function hideMainWindow(): Promise<void> {
  await invoke('hide_main_window')
}

export async function setMainWindowHeight(height: number): Promise<void> {
  await invoke('set_main_window_height', { height })
}

export async function setBarVisible(visible: boolean): Promise<void> {
  await invoke('set_bar_visible', { visible })
}

export async function copyDiagnostics(): Promise<void> {
  await invoke('copy_diagnostics')
}

export async function requestExit(): Promise<void> {
  await invoke('request_exit')
}

export async function listenForSnapshot(
  handler: (snapshot: AppSnapshot) => void,
): Promise<UnlistenFn | undefined> {
  if (!isTauriRuntime()) return undefined
  return listen<SnapshotEvent>('app-snapshot-changed', ({ payload }) => {
    handler('snapshot' in payload ? payload.snapshot : payload)
  })
}

export async function listenForRefreshStatus(
  handler: (event: RefreshStatusEvent) => void,
): Promise<UnlistenFn | undefined> {
  if (!isTauriRuntime()) return undefined
  return listen<RefreshStatusEvent>('refresh-status-changed', ({ payload }) => handler(payload))
}

export async function listenForAuthStatus(
  handler: (auth: AuthStatus) => void,
): Promise<UnlistenFn | undefined> {
  if (!isTauriRuntime()) return undefined
  return listen<AuthStatus>('auth-status-changed', ({ payload }) => handler(payload))
}

export async function listenForSettings(
  handler: (settings: AppSettings) => void,
): Promise<UnlistenFn | undefined> {
  if (!isTauriRuntime()) return undefined
  return listen<AppSettings>('settings-changed', ({ payload }) => handler(payload))
}
