<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import * as echarts from 'echarts/core'
import { PieChart } from 'echarts/charts'
import { LegendComponent, TooltipComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { asChartNumber, moneyDisplay, plainValue, tokenValue } from '../lib/format'
import type { ModelPieSlice } from '../types/contracts'

echarts.use([PieChart, LegendComponent, TooltipComponent, CanvasRenderer])

const props = defineProps<{
  slices: ModelPieSlice[]
  metric: 'tokens' | 'actualCost'
}>()

const usableSlices = computed(() => props.slices.filter((slice) => asChartNumber(slice[props.metric]) !== null))
const root = ref<HTMLElement | null>(null)
let chart: echarts.ECharts | null = null
let resizeObserver: ResizeObserver | null = null
let themeObserver: MutationObserver | null = null

function palette(): { text: string; tooltip: string } {
  const styles = getComputedStyle(document.documentElement)
  return {
    text: styles.getPropertyValue('--chart-text').trim(),
    tooltip: styles.getPropertyValue('--chart-tooltip').trim(),
  }
}

function displaySlice(slice: ModelPieSlice): string {
  return props.metric === 'tokens'
    ? tokenValue(slice.displayTokens ?? slice.tokens)
    : moneyDisplay(slice.displayActualCost, slice.actualCost)
}

function displayLabel(slice: ModelPieSlice): string {
  return plainValue(slice.label)
}

function render(): void {
  if (!root.value) return
  if (!chart) chart = echarts.init(root.value)
  const colors = palette()
  chart.setOption({
    animationDurationUpdate: 180,
    color: ['#93bdd7', '#b7caac', '#e8ce8a', '#d7b2ae', '#bcb5d4', '#a9c8c6'],
    tooltip: {
      trigger: 'item',
      backgroundColor: colors.tooltip,
      borderWidth: 0,
      textStyle: { color: '#ffffff', fontSize: 11 },
      formatter: (params: { data: ModelPieSlice }) => `${displayLabel(params.data)}<br/>${displaySlice(params.data)}`,
    },
    legend: {
      type: 'scroll',
      bottom: 0,
      icon: 'circle',
      itemWidth: 7,
      itemHeight: 7,
      textStyle: { color: colors.text, fontSize: 10 },
    },
    series: [
      {
        type: 'pie',
        radius: ['42%', '72%'],
        center: ['50%', '43%'],
        avoidLabelOverlap: true,
        label: { show: false },
        labelLine: { show: false },
        data: usableSlices.value.map((slice) => ({ ...slice, name: displayLabel(slice), value: asChartNumber(slice[props.metric]) })),
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

watch([usableSlices, () => props.metric], render, { deep: true })

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  themeObserver?.disconnect()
  chart?.dispose()
  chart = null
})
</script>

<template>
  <div v-if="usableSlices.length" ref="root" class="chart-canvas chart-canvas--pie" aria-label="模型分布扇形图" />
  <div v-else class="chart-empty">--</div>
</template>
