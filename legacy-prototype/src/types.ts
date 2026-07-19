export type CollectionStatus = "idle" | "starting" | "collecting" | "error";

export interface MetricAccount {
  balance: number | null;
  apiKeys: number | null;
  enabledKeys: number | null;
  requests: number | null;
  spend: number | null;
  spendLimit: number | null;
  tokens: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  cachedTokens: number | null;
  rpm: number | null;
  tpm: number | null;
  averageLatency: number | null;
}

export interface DistributionRow {
  label: string;
  requests: number | null;
  tokens: number | null;
  spend: number | null;
  standardSpend: number | null;
}

export interface TrendPoint {
  label: string;
  requests: number | null;
  tokens: number | null;
  spend: number | null;
}

export interface UsageRecord {
  id: string;
  apiKey: string | null;
  model: string | null;
  reasoning: string | null;
  endpoint: string | null;
  ip: string | null;
  group: string | null;
  type: string | null;
  billingMode: string | null;
  tokens: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheTokens: number | null;
  spend: number | null;
  standardSpend: number | null;
  latency: number | null;
  time: string | null;
}

export interface DashboardSnapshot {
  account: MetricAccount;
  platforms: DistributionRow[];
  models: DistributionRow[];
  trends: TrendPoint[];
  usage: UsageRecord[];
}

export interface SourceRecord {
  id: string;
  path: string;
  method: string;
  status: number;
  contentType: string;
  bytes: number;
  topLevelKeys: string[];
  seenAt: string;
  count: number;
  sample: unknown;
}

export interface AppState {
  status: CollectionStatus;
  browserUrl: string;
  sessionStartedAt: string | null;
  updatedAt: string | null;
  lastError: string | null;
  snapshot: DashboardSnapshot;
  sources: SourceRecord[];
}
