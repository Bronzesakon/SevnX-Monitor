/**
 * Public values passed through Tauri IPC. They intentionally exclude cookies,
 * tokens, API keys, request identifiers, IP addresses, and complete users.
 */
export type Decimal = number | string
export type Maybe<T> = T | null | undefined

export type AuthStatus = 'loggedOut' | 'loggingIn' | 'validating' | 'authenticated' | 'expired'
export type RefreshStatus = 'idle' | 'refreshing' | 'success' | 'stale' | 'failed'
export type UsageRange =
  | 'today'
  | 'yesterday'
  | 'last24Hours'
  | 'last7Days'
  | 'last14Days'
  | 'last30Days'
  | 'thisMonth'
export type ThemeMode = 'light' | 'dark' | 'system'
export type AlertInterval = '15m' | '30m' | '1h' | '6h'

export interface DisplayValue {
  value?: Maybe<Decimal>
  display?: Maybe<string>
}

export interface DurationValue {
  milliseconds?: Maybe<Decimal>
  display?: Maybe<string>
}

export interface MoneyValue extends DisplayValue {
  status?: Maybe<string>
}

export interface ApiKeySummary {
  total?: Maybe<Decimal>
  active?: Maybe<Decimal>
  displayTotal?: Maybe<string>
  displayActive?: Maybe<string>
}

export interface RequestSummary {
  total?: Maybe<Decimal>
  displayTotal?: Maybe<string>
}

export interface SpendSummary {
  actual?: Maybe<Decimal>
  standard?: Maybe<Decimal>
  displayActual?: Maybe<string>
  displayStandard?: Maybe<string>
}

export interface TokenSummary {
  total?: Maybe<Decimal>
  input?: Maybe<Decimal>
  output?: Maybe<Decimal>
  displayTotal?: Maybe<string>
  displayInput?: Maybe<string>
  displayOutput?: Maybe<string>
}

export interface PerformanceSummary {
  rpm?: Maybe<Decimal>
  tpm?: Maybe<Decimal>
  displayRpm?: Maybe<string>
  displayTpm?: Maybe<string>
}

export interface DashboardSnapshot {
  balance?: Maybe<MoneyValue>
  apiKeys?: Maybe<ApiKeySummary>
  todayRequests?: Maybe<RequestSummary>
  todaySpend?: Maybe<SpendSummary>
  todayTokens?: Maybe<TokenSummary>
  cumulativeTokens?: Maybe<TokenSummary>
  performance?: Maybe<PerformanceSummary>
  averageResponse?: Maybe<DurationValue>
  fetchedAt?: Maybe<string>
}

export interface UsageSummary {
  totalRequests?: Maybe<Decimal>
  totalTokens?: Maybe<Decimal>
  totalInputTokens?: Maybe<Decimal>
  totalOutputTokens?: Maybe<Decimal>
  totalCacheTokens?: Maybe<Decimal>
  totalCacheCreationTokens?: Maybe<Decimal>
  totalCacheReadTokens?: Maybe<Decimal>
  totalActualCost?: Maybe<Decimal>
  totalCost?: Maybe<Decimal>
  averageDurationMs?: Maybe<Decimal>
  displayTotalRequests?: Maybe<string>
  displayTotalTokens?: Maybe<string>
  displayTotalCacheTokens?: Maybe<string>
  displayTotalCacheCreationTokens?: Maybe<string>
  displayTotalCacheReadTokens?: Maybe<string>
  displayTotalActualCost?: Maybe<string>
  displayTotalCost?: Maybe<string>
  displayAverageDurationMs?: Maybe<string>
}

export interface TokenTrendPoint {
  date?: Maybe<string>
  totalTokens?: Maybe<Decimal>
  displayTotalTokens?: Maybe<string>
}

export interface DistributionRow {
  label?: Maybe<string>
  requests?: Maybe<Decimal>
  tokens?: Maybe<Decimal>
  actualCost?: Maybe<Decimal>
  standardCost?: Maybe<Decimal>
  displayRequests?: Maybe<string>
  displayTokens?: Maybe<string>
  displayActualCost?: Maybe<string>
  displayStandardCost?: Maybe<string>
}

export interface ModelPieSlice {
  label?: Maybe<string>
  tokens?: Maybe<Decimal>
  actualCost?: Maybe<Decimal>
  displayTokens?: Maybe<string>
  displayActualCost?: Maybe<string>
  tooltip?: Maybe<Record<string, string>>
}

export interface UsageSnapshot {
  range: UsageRange
  summary?: Maybe<UsageSummary>
  tokenTrend: TokenTrendPoint[]
  models: DistributionRow[]
  groups: DistributionRow[]
  endpoints: DistributionRow[]
  modelPie: ModelPieSlice[]
  fetchedAt?: Maybe<string>
}

export interface PublicError {
  code?: Maybe<string>
  message?: Maybe<string>
}

export interface AppSnapshot {
  auth: AuthStatus
  refresh: RefreshStatus
  dashboard?: Maybe<DashboardSnapshot>
  usage?: Maybe<UsageSnapshot>
  lastSuccessAt?: Maybe<string>
  lastError?: Maybe<PublicError>
}

export interface AppSettings {
  accessUrl: string
  theme: ThemeMode
  autoRefresh: boolean
  lowBalanceAlert: boolean
  lowBalanceThreshold: Decimal
  alertInterval: AlertInterval
  barVisible: boolean
  autostart: boolean
}

export interface RefreshStatusEvent {
  refresh: RefreshStatus
  lastSuccessAt?: Maybe<string>
  lastError?: Maybe<PublicError>
}
