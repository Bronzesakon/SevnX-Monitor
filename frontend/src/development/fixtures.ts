import type { AppSettings, AppSnapshot } from '../types/contracts'

export const developmentSettings: AppSettings = {
  theme: 'system',
  autoRefresh: true,
  lowBalanceAlert: false,
  lowBalanceThreshold: 5,
  alertInterval: '15m',
  barVisible: true,
  autostart: false,
}

/** Browser-only presentation data. The package build has no dependency on this module. */
export const developmentSnapshot: AppSnapshot = {
  auth: 'authenticated',
  refresh: 'success',
  lastSuccessAt: '2026-07-17T13:30:00.000Z',
  dashboard: {
    balance: { value: '18.42', display: '18.42', status: '可用' },
    apiKeys: { total: 3, active: 2 },
    todayRequests: { total: '1,286' },
    todaySpend: { actual: '1.36', standard: '1.62' },
    todayTokens: { total: '1.48M', input: '1.05M', output: '430K' },
    cumulativeTokens: { total: '18.7M', input: '14.1M', output: '4.6M' },
    performance: { rpm: 24, tpm: '36.2K' },
    averageResponse: { milliseconds: 682, display: '682 ms' },
  },
  usage: {
    range: 'last24Hours',
    summary: {
      totalRequests: '1,286',
      totalTokens: '1.48M',
      totalInputTokens: '1.05M',
      totalOutputTokens: '430K',
      totalCacheTokens: '116K',
      totalCacheCreationTokens: '12K',
      totalCacheReadTokens: '104K',
      totalActualCost: '1.36',
      totalCost: '1.62',
      averageDurationMs: 682,
      displayAverageDurationMs: '682 ms',
    },
    tokenTrend: [
      { date: '00:00', totalTokens: 28400, inputTokens: 12500, outputTokens: 3900, cacheCreationTokens: 1000, cacheReadTokens: 11000, actualCost: '0.03', cost: '0.08' },
      { date: '04:00', totalTokens: 62100, inputTokens: 28100, outputTokens: 8200, cacheCreationTokens: 2400, cacheReadTokens: 23400, actualCost: '0.09', cost: '0.21' },
      { date: '08:00', totalTokens: 186400, inputTokens: 75200, outputTokens: 29700, cacheCreationTokens: 8300, cacheReadTokens: 73200, actualCost: '0.34', cost: '0.61' },
      { date: '12:00', totalTokens: 254600, inputTokens: 102500, outputTokens: 40800, cacheCreationTokens: 11200, cacheReadTokens: 100100, actualCost: '0.48', cost: '0.82' },
      { date: '16:00', totalTokens: 202300, inputTokens: 82500, outputTokens: 32100, cacheCreationTokens: 7600, cacheReadTokens: 80100, actualCost: '0.38', cost: '0.68' },
      { date: '20:00', totalTokens: 158700, inputTokens: 64300, outputTokens: 24900, cacheCreationTokens: 5900, cacheReadTokens: 63600, actualCost: '0.29', cost: '0.51' },
    ],
    models: [
      { label: 'claude-sonnet-4', requests: 534, tokens: '612K', actualCost: '0.68', standardCost: '0.80' },
      { label: 'gpt-4.1', requests: 428, tokens: '498K', actualCost: '0.42', standardCost: '0.51' },
      { label: 'gemini-2.5-pro', requests: 324, tokens: '370K', actualCost: '0.26', standardCost: '0.31' },
    ],
    groups: [
      { label: '默认分组', requests: 860, tokens: '982K', actualCost: '0.92', standardCost: '1.08' },
      { label: '研发', requests: 426, tokens: '498K', actualCost: '0.44', standardCost: '0.54' },
    ],
    endpoints: [
      { label: '/v1/chat/completions', requests: 1114, tokens: '1.31M', actualCost: '1.21', standardCost: '1.44' },
      { label: '/v1/responses', requests: 172, tokens: '170K', actualCost: '0.15', standardCost: '0.18' },
    ],
    modelPie: [
      { label: 'claude-sonnet-4', tokens: 612000, actualCost: '0.68' },
      { label: 'gpt-4.1', tokens: 498000, actualCost: '0.42' },
      { label: 'gemini-2.5-pro', tokens: 370000, actualCost: '0.26' },
    ],
  },
}
