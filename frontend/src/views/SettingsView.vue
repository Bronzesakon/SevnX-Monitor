<script setup lang="ts">
import { ref, watch } from 'vue'
import AppIcon from '../components/AppIcon.vue'
import ToggleSwitch from '../components/ToggleSwitch.vue'
import { useAppStore } from '../stores/app'
import type { ShortcutLocation } from '../lib/ipc'
import type { AlertInterval, ThemeMode } from '../types/contracts'

const app = useAppStore()
const threshold = ref(String(app.settings.lowBalanceThreshold))
const shortcutLocation = ref<ShortcutLocation>('desktop')

watch(() => app.settings.lowBalanceThreshold, (value) => {
  threshold.value = String(value)
})

function updateTheme(event: Event): void {
  app.saveSettings({ theme: (event.target as HTMLSelectElement).value as ThemeMode })
}

function updateThreshold(): void {
  const value = Number(threshold.value)
  if (!Number.isFinite(value) || value < 0) {
    threshold.value = String(app.settings.lowBalanceThreshold)
    return
  }
  app.saveSettings({ lowBalanceThreshold: value })
}

function updateInterval(event: Event): void {
  app.saveSettings({ alertInterval: (event.target as HTMLSelectElement).value as AlertInterval })
}
</script>

<template>
  <main class="settings-view section-surface" aria-label="设置">
    <div class="settings-titlebar">
      <button class="back-button" type="button" @click="app.activeView = 'dashboard'">
        <AppIcon name="back" :size="17" />
        <span>返回</span>
      </button>
      <h1>设置</h1>
      <div class="shortcut-toolbar">
        <select v-model="shortcutLocation" class="shortcut-location" aria-label="快捷方式位置">
          <option value="desktop">桌面</option>
          <option value="startMenu">开始菜单</option>
        </select>
        <button class="shortcut-button" type="button" title="在所选位置创建可注入状态横条的 Codex 快捷方式" @click="app.createCodexShortcut(shortcutLocation)">
          <AppIcon name="codex" :size="15" />
          <span>创建快捷方式</span>
        </button>
      </div>
    </div>

    <section class="settings-section">
      <p class="settings-section__label">显示与刷新</p>
      <div class="settings-list">
        <label class="settings-row settings-row--url">
          <span><strong>访问网址 URL</strong><small>用于登录、打开 Dashboard 和数据请求</small></span>
          <input
            class="url-input"
            :value="app.settings.accessUrl"
            type="url"
            placeholder="https://www.sevnx.lol"
            spellcheck="false"
            @change="app.saveSettings({ accessUrl: ($event.target as HTMLInputElement).value.trim() })"
          />
        </label>
        <label class="settings-row">
          <span><strong>主题</strong><small>选择界面外观</small></span>
          <select :value="app.settings.theme" @change="updateTheme">
            <option value="light">浅色</option>
            <option value="dark">深色</option>
            <option value="system">跟随系统</option>
          </select>
        </label>
        <div class="settings-row">
          <span><strong>显示吸附横条</strong><small>横条始终置顶，可拖动吸附</small></span>
          <ToggleSwitch :model-value="app.settings.barVisible" label="显示吸附横条" @update:model-value="app.saveSettings({ barVisible: $event })" />
        </div>
        <div class="settings-row">
          <span><strong>开机自动启动</strong><small>默认关闭</small></span>
          <ToggleSwitch :model-value="app.settings.autostart" label="开机自动启动" @update:model-value="app.saveSettings({ autostart: $event })" />
        </div>
      </div>
    </section>

    <section class="settings-section">
      <p class="settings-section__label">低余额提醒</p>
      <div class="settings-list">
        <div class="settings-row">
          <span><strong>低余额提醒</strong><small>仅发送 Windows 系统通知</small></span>
          <ToggleSwitch :model-value="app.settings.lowBalanceAlert" label="低余额提醒" @update:model-value="app.saveSettings({ lowBalanceAlert: $event })" />
        </div>
        <label class="settings-row">
          <span><strong>余额阈值</strong><small>余额小于或等于此值时提醒</small></span>
          <span class="number-input"><b>¥</b><input v-model="threshold" type="number" min="0" step="any" inputmode="decimal" @change="updateThreshold" /></span>
        </label>
        <label class="settings-row">
          <span><strong>重复间隔</strong><small>余额持续偏低时再次提醒</small></span>
          <select :value="app.settings.alertInterval" :disabled="!app.settings.lowBalanceAlert" @change="updateInterval">
            <option value="15m">15 分钟</option>
            <option value="30m">30 分钟</option>
            <option value="1h">1 小时</option>
            <option value="6h">6 小时</option>
          </select>
        </label>
      </div>
    </section>

    <section class="settings-section">
      <p class="settings-section__label">账户与诊断</p>
      <div class="settings-list settings-list--actions">
        <button type="button" class="settings-action" @click="app.testRefreshToken"><span><strong>测试令牌续期</strong><small>触发一次 Refresh Token 刷新，写入日志供分析</small></span><AppIcon name="refresh" /></button>
        <button type="button" class="settings-action" @click="app.login"><span><strong>重新登录</strong><small>在应用内重新打开登录窗口</small></span><AppIcon name="login" /></button>
        <button type="button" class="settings-action" @click="app.openOfficialDashboard"><span><strong>打开 SevnX Dashboard</strong><small>使用系统默认浏览器</small></span><AppIcon name="external" /></button>
        <button type="button" class="settings-action" @click="app.openLogFile"><span><strong>打开日志文件</strong><small>用系统默认方式打开最新日志</small></span><AppIcon name="copy" /></button>
      </div>
    </section>
  </main>
</template>
