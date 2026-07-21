<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import * as echarts from 'echarts/core'
import { PieChart } from 'echarts/charts'
import { TooltipComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { asChartNumber, moneyDisplay, plainValue, tokenValue } from '../lib/format'
import type { ModelPieSlice } from '../types/contracts'

echarts.use([PieChart, TooltipComponent, CanvasRenderer])

const PIE_COLORS = ['#93bdd7', '#b7caac', '#e8ce8a', '#d7b2ae', '#bcb5d4', '#a9c8c6']

const props = defineProps<{
  slices: ModelPieSlice[]
  metric: 'tokens' | 'actualCost'
}>()

const usableSlices = computed(() => props.slices.filter((slice) => asChartNumber(slice[props.metric]) !== null))
const legendItems = computed(() => usableSlices.value.map((slice, index) => ({
  slice,
  index,
  color: PIE_COLORS[index % PIE_COLORS.length],
})))
const root = ref<HTMLElement | null>(null)
let chart: echarts.ECharts | null = null
let resizeObserver: ResizeObserver | null = null
let themeObserver: MutationObserver | null = null
const highlightedIndex = ref<number | null>(null)

function palette(): { tooltip: string } {
  const styles = getComputedStyle(document.documentElement)
  return { tooltip: styles.getPropertyValue('--chart-tooltip').trim() }
}

function displaySlice(slice: ModelPieSlice): string {
  return props.metric === 'tokens'
    ? tokenValue(slice.displayTokens ?? slice.tokens)
    : moneyDisplay(slice.displayActualCost, slice.actualCost)
}

function displayLabel(slice: ModelPieSlice): string {
  return plainValue(slice.label)
}

function highlight(index: number): void {
  if (!chart) return
  if (highlightedIndex.value !== null && highlightedIndex.value !== index) {
    chart.dispatchAction({ type: 'downplay', seriesIndex: 0, dataIndex: highlightedIndex.value })
  }
  chart.dispatchAction({ type: 'highlight', seriesIndex: 0, dataIndex: index })
  highlightedIndex.value = index
}

function clearHighlight(index: number): void {
  if (!chart || highlightedIndex.value !== index) return
  chart.dispatchAction({ type: 'downplay', seriesIndex: 0, dataIndex: index })
  highlightedIndex.value = null
}

function render(): void {
  if (!root.value) return
  if (!chart) chart = echarts.init(root.value)
  const colors = palette()
  highlightedIndex.value = null
  chart.setOption({
    animationDurationUpdate: 180,
    color: PIE_COLORS,
    tooltip: {
      trigger: 'item',
      backgroundColor: colors.tooltip,
      borderWidth: 0,
      textStyle: { color: '#ffffff', fontSize: 11 },
      formatter: (params: { data: ModelPieSlice }) => `${displayLabel(params.data)}<br/>${displaySlice(params.data)}`,
    },
    series: [
      {
        type: 'pie',
        radius: ['42%', '72%'],
        center: ['50%', '50%'],
        avoidLabelOverlap: true,
        label: { show: false },
        labelLine: { show: false },
        emphasis: { scale: true, scaleSize: 6 },
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

watch([usableSlices, () => props.metric], async () => {
  await nextTick()
  render()
}, { deep: true })

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  themeObserver?.disconnect()
  chart?.dispose()
  chart = null
})
</script>

<template>
  <div v-if="usableSlices.length" class="model-pie-layout">
    <div class="model-pie-legend" role="list" aria-label="模型分布图例">
      <button
        v-for="item in legendItems"
        :key="`${displayLabel(item.slice)}-${item.index}`"
        class="model-pie-legend__item"
        type="button"
        role="listitem"
        :title="displayLabel(item.slice)"
        @mouseenter="highlight(item.index)"
        @mouseleave="clearHighlight(item.index)"
        @focus="highlight(item.index)"
        @blur="clearHighlight(item.index)"
      >
        <span class="model-pie-legend__dot" :style="{ backgroundColor: item.color }" aria-hidden="true" />
        <span class="model-pie-legend__label">{{ displayLabel(item.slice) }}</span>
      </button>
    </div>
    <div ref="root" class="chart-canvas chart-canvas--pie" aria-label="模型分布扇形图" />
  </div>
  <div v-else class="chart-empty">--</div>
</template>
