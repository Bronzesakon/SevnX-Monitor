<script setup lang="ts">
import AppIcon, { type IconName } from './AppIcon.vue'

defineProps<{
  icon: IconName
  label: string
  active?: boolean
  spinning?: boolean
  disabled?: boolean
}>()

defineEmits<{
  click: []
}>()
</script>

<template>
  <button
    class="icon-button"
    :class="{ 'is-active': active, 'is-spinning': spinning }"
    type="button"
    :title="label"
    :aria-label="label"
    :disabled="disabled"
    @click="$emit('click')"
  >
    <AppIcon :name="icon" />
  </button>
</template>
