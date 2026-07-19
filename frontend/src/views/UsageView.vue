<script setup lang="ts">
import { computed, ref } from 'vue'
import DistributionTable from '../components/DistributionTable.vue'
import MetricCard from '../components/MetricCard.vue'
import ModelPieChart from '../components/ModelPieChart.vue'
import TokenTrendChart from '../components/TokenTrendChart.vue'
import { durationValue, moneyDisplay, plainValue, tokenValue } from '../lib/format'
import { useAppStore, type UsagePanel } from '../stores/app'
import type { UsageRange, UsageSnapshot } from '../types/contracts'

const props = defineProps<{
  usage?: UsageSnapshot | null
}>()

const app = useAppStore()
const pieMetric = ref<'tokens' | 'actualCost'>('tokens')

const ranges: Array<{ value: UsageRange; label: string }> = [
  { value: 'today', label: '今天' },
  { value: 'yesterday', label: '昨天' },
  { value: 'last24Hours', label: '近 24 小时' },
  { value: 'last7Days', label: '近 7 天' },
  { value: 'last14Days', label: '近 14 天' },
  { value: 'last30Days', label: '近 30 天' },
  { value: 'thisMonth', label: '本月' },
]

const panels: Array<{ value: UsagePanel; label: string }> = [
  { value: 'trend', label: 'Token 趋势' },
  { value: 'models', label: '模型' },
  { value: 'groups', label: '分组' },
  { value: 'endpoints', label: '端点' },
]

const summaryCards = computed(() => {
  const summary = props.usage?.summary
  return [
    { label: '总请求数', value: plainValue(summary?.displayTotalRequests ?? summary?.totalRequests), details: ['所选范围内'], tone: 'blue' as const, icon: 'chart' as const },
    {
      label: '总 Token',
      value: tokenValue(summary?.displayTotalTokens ?? summary?.totalTokens),
      details: [
        `输入 ${tokenValue(summary?.totalInputTokens)}`,
        `输出 ${tokenValue(summary?.totalOutputTokens)}`,
        `缓存 ${tokenValue(summary?.displayTotalCacheTokens ?? summary?.totalCacheTokens)}`,
        `创建 ${tokenValue(summary?.displayTotalCacheCreationTokens ?? summary?.totalCacheCreationTokens)}`,
        `读取 ${tokenValue(summary?.displayTotalCacheReadTokens ?? summary?.totalCacheReadTokens)}`,
      ],
      tone: 'yellow' as const,
      icon: 'cube' as const,
    },
    {
      label: '总消费',
      value: moneyDisplay(summary?.displayTotalActualCost, summary?.totalActualCost),
      details: [`标准 ${moneyDisplay(summary?.displayTotalCost, summary?.totalCost)}`],
      tone: 'green' as const,
      icon: 'coin' as const,
    },
    {
      label: '平均耗时',
      value: durationValue(summary?.displayAverageDurationMs, summary?.averageDurationMs),
      details: ['网页格式'],
      tone: 'plain' as const,
      icon: 'clock' as const,
    },
  ]
})

function updateRange(event: Event): void {
  app.selectUsageRange((event.target as HTMLSelectElement).value as UsageRange)
}
</script>

<template>
  <main class="usage-view section-surface" aria-label="使用记录">
    <div class="usage-toolbar">
      <div class="usage-panels" role="tablist" aria-label="使用记录子页面">
        <button
          v-for="panel in panels"
          :key="panel.value"
          class="segment-button"
          :class="{ 'is-active': app.usagePanel === panel.value }"
          type="button"
          role="tab"
          :aria-selected="app.usagePanel === panel.value"
          @click="app.usagePanel = panel.value"
        >
          {{ panel.label }}
        </button>
      </div>
      <label class="range-select">
        <span class="sr-only">时间范围</span>
        <select
          :value="usage?.range ?? 'last24Hours'"
          :disabled="app.pending || app.refresh === 'refreshing' || app.auth === 'validating'"
          @change="updateRange"
        >
          <option v-for="range in ranges" :key="range.value" :value="range.value">{{ range.label }}</option>
        </select>
      </label>
    </div>

    <div class="usage-summary-grid">
      <MetricCard
        v-for="card in summaryCards"
        :key="card.label"
        :label="card.label"
        :value="card.value"
        :details="card.details"
        :wrap-details="card.label === '总 Token'"
        :tone="card.tone"
        :icon="card.icon"
      />
    </div>

    <section v-if="app.usagePanel === 'trend'" class="usage-panel">
      <div class="panel-heading">
        <div>
          <p class="eyebrow">总 Token</p>
          <h2>总 Token</h2>
        </div>
      </div>
      <TokenTrendChart :points="usage?.tokenTrend ?? []" />
    </section>

    <section v-else-if="app.usagePanel === 'models'" class="usage-panel usage-panel--model">
      <div class="panel-heading panel-heading--model">
        <div>
          <p class="eyebrow">模型分布</p>
          <h2>模型分布</h2>
        </div>
        <div class="compact-toggle" aria-label="模型扇形图统计方式">
          <button :class="{ 'is-active': pieMetric === 'tokens' }" type="button" @click="pieMetric = 'tokens'">按 Token</button>
          <button :class="{ 'is-active': pieMetric === 'actualCost' }" type="button" @click="pieMetric = 'actualCost'">按实际消费</button>
        </div>
      </div>
      <ModelPieChart :slices="usage?.modelPie ?? []" :metric="pieMetric" />
      <DistributionTable dimension-label="模型" :rows="usage?.models ?? []" />
    </section>

    <section v-else-if="app.usagePanel === 'groups'" class="usage-panel">
      <div class="panel-heading">
        <div>
          <p class="eyebrow">分组分布</p>
          <h2>分组分布</h2>
        </div>
      </div>
      <DistributionTable dimension-label="分组" :rows="usage?.groups ?? []" />
    </section>

    <section v-else class="usage-panel">
      <div class="panel-heading">
        <div>
          <p class="eyebrow">端点分布</p>
          <h2>端点分布</h2>
        </div>
      </div>
      <DistributionTable dimension-label="端点" :rows="usage?.endpoints ?? []" />
    </section>
  </main>
</template>
