<script setup lang="ts">
import { computed } from 'vue'
import { lastUpdated } from '../lib/format'
import type { Maybe, PublicError, RefreshStatus } from '../types/contracts'

const props = defineProps<{
  refresh: RefreshStatus
  lastSuccessAt?: Maybe<string>
  error?: Maybe<PublicError>
}>()

function successfulRefreshText(): string {
  return props.lastSuccessAt ? `上次成功刷新：${lastUpdated(props.lastSuccessAt)}` : '暂无成功数据'
}

const message = computed(() => {
  if (props.refresh === 'refreshing') return '正在刷新数据'
  if (props.refresh === 'stale') {
    const failure = props.error?.message ? `刷新失败 · ${props.error.message}` : '刷新失败'
    return `${failure} · ${successfulRefreshText()}`
  }
  if (props.refresh === 'failed') {
    return props.error?.message ? `刷新失败 · ${props.error.message}` : '刷新失败'
  }
  return props.lastSuccessAt ? successfulRefreshText() : '等待首次刷新'
})
</script>

<template>
  <div class="status-strip" :class="`status-strip--${refresh}`" role="status" aria-live="polite">
    <span class="status-strip__dot" />
    <span class="status-strip__message">{{ message }}</span>
    <span v-if="refresh === 'stale'" class="status-badge">旧数据</span>
  </div>
</template>
