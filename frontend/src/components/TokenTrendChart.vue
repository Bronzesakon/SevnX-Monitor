<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import * as echarts from 'echarts/core'
import { LineChart } from 'echarts/charts'
import { GridComponent, TooltipComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { asChartNumber, plainValue, tokenValue } from '../lib/format'
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

function render(): void {
  if (!root.value) return
  if (!chart) chart = echarts.init(root.value)
  const colors = palette()
  chart.setOption({
    animationDurationUpdate: 180,
    grid: { left: 4, right: 8, top: 20, bottom: 24, containLabel: true },
    xAxis: {
      type: 'category',
      boundaryGap: false,
      data: props.points.map((point) => plainValue(point.date)),
      axisLine: { lineStyle: { color: colors.line } },
      axisTick: { show: false },
      axisLabel: { color: colors.text, fontSize: 10, hideOverlap: true },
    },
    yAxis: {
      type: 'value',
      splitNumber: 3,
      axisLabel: { color: colors.text, fontSize: 10 },
      splitLine: { lineStyle: { color: colors.line } },
    },
    tooltip: {
      trigger: 'axis',
      backgroundColor: colors.tooltip,
      borderWidth: 0,
      textStyle: { color: '#ffffff', fontSize: 11 },
      valueFormatter: (value: string | number) => tokenValue(value),
    },
    series: [
      {
        type: 'line',
        data: props.points.map((point) => asChartNumber(point.totalTokens)),
        showSymbol: false,
        smooth: false,
        lineStyle: { width: 2, color: colors.accent },
        itemStyle: { color: colors.accent },
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
