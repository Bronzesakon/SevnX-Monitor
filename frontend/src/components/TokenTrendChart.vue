<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import * as echarts from 'echarts/core'
import { LineChart } from 'echarts/charts'
import { GridComponent, TooltipComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { asChartNumber, moneyValue, plainValue, tokenValue } from '../lib/format'
import type { TokenTrendPoint } from '../types/contracts'

echarts.use([LineChart, GridComponent, TooltipComponent, CanvasRenderer])

const props = defineProps<{
  points: TokenTrendPoint[]
}>()

const hasTrendData = computed(() => props.points.some((point) => asChartNumber(point.totalTokens) !== null))
const root = ref<HTMLElement | null>(null)
let chart: echarts.ECharts | null = null
let resizeObserver: ResizeObserver | null = null
let themeObserver: MutationObserver | null = null

function palette(): { text: string; line: string; accent: string; tooltip: string } {
  const styles = getComputedStyle(document.documentElement)
  return {
    text: styles.getPropertyValue('--chart-text').trim(),
    line: styles.getPropertyValue('--chart-line').trim(),
    accent: styles.getPropertyValue('--chart-accent').trim(),
    tooltip: styles.getPropertyValue('--chart-tooltip').trim(),
  }
}

function cacheHitRate(point: TokenTrendPoint): number | null {
  const total = asChartNumber(point.totalTokens)
  const cacheRead = asChartNumber(point.cacheReadTokens)
  if (total === null || cacheRead === null || total <= 0 || cacheRead < 0) return null
  return Math.min(100, Math.max(0, (cacheRead / total) * 100))
}

function tooltipValue(point: TokenTrendPoint, label: string, value: unknown, formatter: (raw: unknown) => string): string {
  return `<div class="trend-tooltip__row"><span>${label}</span><strong>${formatter(value)}</strong></div>`
}

function tooltipHtml(index: number): string {
  const point = props.points[index]
  if (!point) return ''
  const rate = cacheHitRate(point)
  return [
    `<div class="trend-tooltip__title">${plainValue(point.date)}</div>`,
    tooltipValue(point, '总 Token', point.totalTokens, tokenValue),
    tooltipValue(point, '输入', point.inputTokens, tokenValue),
    tooltipValue(point, '输出', point.outputTokens, tokenValue),
    tooltipValue(point, '缓存创建', point.cacheCreationTokens, tokenValue),
    tooltipValue(point, '缓存读取', point.cacheReadTokens, tokenValue),
    tooltipValue(point, '缓存命中率', rate, (value) => value === null ? '--' : `${Number(value).toFixed(1)}%`),
    tooltipValue(point, '实际消费', point.actualCost, moneyValue),
    tooltipValue(point, '标准消费', point.cost, moneyValue),
  ].join('')
}

function render(): void {
  if (!root.value) return
  if (!chart) chart = echarts.init(root.value)
  const colors = palette()
  chart.setOption({
    animationDurationUpdate: 180,
    grid: { left: 4, right: 34, top: 20, bottom: 24, containLabel: true },
    xAxis: {
      type: 'category',
      boundaryGap: false,
      data: props.points.map((point) => plainValue(point.date)),
      axisLine: { lineStyle: { color: colors.line } },
      axisTick: { show: false },
      axisLabel: { color: colors.text, fontSize: 10, hideOverlap: true },
    },
    yAxis: [
      {
        type: 'value',
        splitNumber: 3,
        axisLabel: { color: colors.text, fontSize: 10, formatter: (value: number) => tokenValue(value) },
        splitLine: { lineStyle: { color: colors.line } },
      },
      {
        type: 'value',
        min: 0,
        max: 100,
        interval: 50,
        axisLabel: { color: colors.text, fontSize: 10, formatter: '{value}%' },
        splitLine: { show: false },
      },
    ],
    tooltip: {
      trigger: 'axis',
      backgroundColor: colors.tooltip,
      borderWidth: 0,
      textStyle: { color: '#ffffff', fontSize: 11 },
      formatter: (params: { dataIndex: number }[]) => tooltipHtml(params[0]?.dataIndex ?? -1),
    },
    series: [
      {
        name: '总 Token',
        type: 'line',
        data: props.points.map((point) => asChartNumber(point.totalTokens)),
        showSymbol: false,
        smooth: false,
        lineStyle: { width: 2, color: colors.accent },
        itemStyle: { color: colors.accent },
        emphasis: { focus: 'series' },
      },
      {
        name: '缓存命中率',
        type: 'line',
        yAxisIndex: 1,
        data: props.points.map(cacheHitRate),
        showSymbol: false,
        smooth: false,
        lineStyle: { width: 2, type: 'dashed', color: '#7b61ff' },
        itemStyle: { color: '#7b61ff' },
        emphasis: { focus: 'series' },
      },
    ],
  }, { notMerge: true })
}

onMounted(async () => {
  await nextTick()
  render()
  resizeObserver = new ResizeObserver(() => chart?.resize())
  if (root.value) resizeObserver.observe(root.value)
  themeObserver = new MutationObserver(render)
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
})

watch(() => props.points, render, { deep: true })

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  themeObserver?.disconnect()
  chart?.dispose()
  chart = null
})
</script>

<template>
  <div v-if="hasTrendData" ref="root" class="chart-canvas" aria-label="总 Token 趋势图" />
  <div v-else class="chart-empty">--</div>
</template>
