<script setup lang="ts">
const value = defineModel<boolean>({ required: true })

defineProps<{
  label: string
}>()
</script>

<template>
  <label class="toggle-switch">
    <span class="sr-only">{{ label }}</span>
    <input v-model="value" type="checkbox" />
    <span class="toggle-switch__track" aria-hidden="true"><span class="toggle-switch__thumb" /></span>
  </label>
</template>
