<script setup lang="ts">
import AppIcon, { type IconName } from './AppIcon.vue'

defineProps<{
  label: string
  value: string
  details?: string[]
  secondaryLabel?: string
  secondaryValue?: string
  secondaryDetails?: string[]
  secondaryTone?: 'plain' | 'blue' | 'green' | 'yellow' | 'red' | 'purple' | 'indigo' | 'violet'
  wrapDetails?: boolean
  tone?: 'plain' | 'blue' | 'green' | 'yellow' | 'red' | 'purple' | 'indigo' | 'violet'
  icon?: IconName
}>()
</script>

<template>
  <article class="metric-card" :class="[`metric-card--${tone ?? 'plain'}`, { 'metric-card--grouped': secondaryLabel, 'metric-card--wrap-details': wrapDetails }]">
    <span v-if="icon" class="metric-card__icon"><AppIcon :name="icon" :size="20" /></span>
    <div class="metric-card__content">
      <p class="metric-card__label">{{ label }}</p>
      <p class="metric-card__value" :title="value">{{ value }}</p>
      <div v-if="details?.length" class="metric-card__details" :class="{ 'metric-card__details--wrap': wrapDetails }">
        <span v-for="detail in details" :key="detail" :title="detail">{{ detail }}</span>
      </div>
    </div>
    <div v-if="secondaryLabel" class="metric-card__content metric-card__content--secondary" :class="secondaryTone ? `metric-card__content--${secondaryTone}` : undefined">
      <p class="metric-card__label">{{ secondaryLabel }}</p>
      <p class="metric-card__value" :title="secondaryValue">{{ secondaryValue }}</p>
      <div v-if="secondaryDetails?.length" class="metric-card__details">
        <span v-for="detail in secondaryDetails" :key="detail" :title="detail">{{ detail }}</span>
      </div>
    </div>
  </article>
</template>
