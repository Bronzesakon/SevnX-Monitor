<script setup lang="ts">
import { computed } from 'vue'
import MetricCard from '../components/MetricCard.vue'
import { durationValue, moneyDisplay, plainValue, tokenValue } from '../lib/format'
import type { DashboardSnapshot } from '../types/contracts'

const props = defineProps<{
  dashboard?: DashboardSnapshot | null
}>()

const featuredCards = computed(() => {
  const data = props.dashboard
  return [
    {
      label: '余额',
      value: moneyDisplay(data?.balance?.display, data?.balance?.value),
      details: data?.balance?.status ? [data.balance.status] : [],
      tone: 'green' as const,
      icon: 'wallet' as const,
      secondaryLabel: '今日消费',
      secondaryValue: moneyDisplay(data?.todaySpend?.displayActual, data?.todaySpend?.actual),
      secondaryDetails: [`标准 ${moneyDisplay(data?.todaySpend?.displayStandard, data?.todaySpend?.standard)}`],
      secondaryTone: 'purple' as const,
    },
    {
      label: '今日 Token',
      value: tokenValue(data?.todayTokens?.displayTotal ?? data?.todayTokens?.total),
      details: [
        `输入 ${tokenValue(data?.todayTokens?.displayInput ?? data?.todayTokens?.input)}`,
        `输出 ${tokenValue(data?.todayTokens?.displayOutput ?? data?.todayTokens?.output)}`,
      ],
      tone: 'yellow' as const,
      icon: 'cube' as const,
      secondaryLabel: '累计 Token',
      secondaryValue: tokenValue(data?.cumulativeTokens?.displayTotal ?? data?.cumulativeTokens?.total),
      secondaryDetails: [
        `输入 ${tokenValue(data?.cumulativeTokens?.displayInput ?? data?.cumulativeTokens?.input)}`,
        `输出 ${tokenValue(data?.cumulativeTokens?.displayOutput ?? data?.cumulativeTokens?.output)}`,
      ],
    },
  ]
})

const cards = computed(() => {
  const data = props.dashboard
  return [
    {
      label: 'API 密钥',
      value: plainValue(data?.apiKeys?.displayTotal ?? data?.apiKeys?.total),
      details: [`已启用 ${plainValue(data?.apiKeys?.displayActive ?? data?.apiKeys?.active)}`],
      tone: 'plain' as const,
      icon: 'key' as const,
    },
    {
      label: '今日请求',
      value: plainValue(data?.todayRequests?.displayTotal ?? data?.todayRequests?.total),
      details: ['网页返回总计'],
      tone: 'green' as const,
      icon: 'chart' as const,
    },
    {
      label: '性能指标',
      value: `RPM ${plainValue(data?.performance?.displayRpm ?? data?.performance?.rpm)}`,
      details: [`TPM ${plainValue(data?.performance?.displayTpm ?? data?.performance?.tpm)}`],
      tone: 'violet' as const,
      icon: 'bolt' as const,
    },
    {
      label: '平均响应',
      value: durationValue(data?.averageResponse?.display, data?.averageResponse?.milliseconds),
      details: ['平均时间'],
      tone: 'red' as const,
      icon: 'clock' as const,
    },
  ]
})
</script>

<template>
  <main class="dashboard-view section-surface" aria-label="仪表盘">
    <div class="metric-grid metric-grid--featured">
      <MetricCard
        v-for="card in featuredCards"
        :key="card.label"
        :label="card.label"
        :value="card.value"
        :details="card.details"
        :secondary-label="card.secondaryLabel"
        :secondary-value="card.secondaryValue"
        :secondary-details="card.secondaryDetails"
        :secondary-tone="card.secondaryTone"
        :tone="card.tone"
        :icon="card.icon"
      />
    </div>
    <div class="metric-grid metric-grid--secondary">
      <MetricCard
        v-for="card in cards"
        :key="card.label"
        :label="card.label"
        :value="card.value"
        :details="card.details"
        :tone="card.tone"
        :icon="card.icon"
      />
    </div>
  </main>
</template>
