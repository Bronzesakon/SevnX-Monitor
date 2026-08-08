<script setup lang="ts">
import { computed } from 'vue'
import { compactTokenValue, moneyDisplay } from '../lib/format'
import type { DashboardSnapshot } from '../types/contracts'

const props = defineProps<{
  dashboard?: DashboardSnapshot | null
}>()

const values = computed(() => [
  {
    label: '余额',
    value: moneyDisplay(props.dashboard?.balance?.display, props.dashboard?.balance?.value),
  },
  {
    label: '今日消费',
    value: moneyDisplay(props.dashboard?.todaySpend?.displayActual, props.dashboard?.todaySpend?.actual),
  },
  {
    label: '今日 Token',
    value: compactTokenValue(props.dashboard?.todayTokens?.displayTotal ?? props.dashboard?.todayTokens?.total),
  },
])

</script>

<template>
  <main class="bar-view" aria-label="账户摘要横条">
    <div
      class="bar-view__surface"
      data-tauri-drag-region
    >
      <span class="bar-view__items">
        <span v-for="item in values" :key="item.label" class="bar-view__item">
          <span class="bar-view__label">{{ item.label }}</span>
          <span class="bar-view__value" :title="item.value">{{ item.value }}</span>
        </span>
      </span>
    </div>
  </main>
</template>
