<script setup lang="ts">
import { computed } from 'vue'
import { asChartNumber, moneyDisplay, plainValue, tokenValue } from '../lib/format'
import type { DistributionRow } from '../types/contracts'

const props = defineProps<{
  dimensionLabel: '模型' | '分组' | '端点'
  rows: DistributionRow[]
}>()

const sortedRows = computed(() => props.rows
  .map((row, index) => ({ row, index }))
  .sort((left, right) => {
    const leftValue = asChartNumber(left.row.actualCost)
    const rightValue = asChartNumber(right.row.actualCost)

    if (leftValue === null && rightValue === null) return left.index - right.index
    if (leftValue === null) return 1
    if (rightValue === null) return -1
    return rightValue - leftValue || left.index - right.index
  })
  .map(({ row }) => row))
</script>

<template>
  <div class="distribution-table-wrap">
    <table v-if="sortedRows.length" class="distribution-table">
      <colgroup>
        <col class="distribution-table__dimension" />
        <col class="distribution-table__requests" />
        <col class="distribution-table__tokens" />
        <col class="distribution-table__cost" />
        <col class="distribution-table__cost" />
      </colgroup>
      <thead>
        <tr>
          <th :title="dimensionLabel">{{ dimensionLabel }}</th>
          <th>请求</th>
          <th>Token</th>
          <th>实际</th>
          <th>标准</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(row, index) in sortedRows" :key="row.label ?? `row-${index}`">
          <td :title="plainValue(row.label)">{{ plainValue(row.label) }}</td>
          <td :title="plainValue(row.displayRequests ?? row.requests)">{{ plainValue(row.displayRequests ?? row.requests) }}</td>
          <td :title="tokenValue(row.displayTokens ?? row.tokens)">{{ tokenValue(row.displayTokens ?? row.tokens) }}</td>
          <td :title="moneyDisplay(row.displayActualCost, row.actualCost)">{{ moneyDisplay(row.displayActualCost, row.actualCost) }}</td>
          <td :title="moneyDisplay(row.displayStandardCost, row.standardCost)">{{ moneyDisplay(row.displayStandardCost, row.standardCost) }}</td>
        </tr>
      </tbody>
    </table>
    <div v-else class="table-empty">--</div>
  </div>
</template>
