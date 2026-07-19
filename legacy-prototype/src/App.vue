<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import type { AppState, SourceRecord, UsageRecord } from "./types";

const createInitialState = (): AppState => ({
  status: "idle",
  browserUrl: "https://www.sevnx.one/dashboard",
  sessionStartedAt: null,
  updatedAt: null,
  lastError: null,
  snapshot: {
    account: {
      balance: null,
      apiKeys: null,
      enabledKeys: null,
      requests: null,
      spend: null,
      spendLimit: null,
      tokens: null,
      inputTokens: null,
      outputTokens: null,
      cachedTokens: null,
      rpm: null,
      tpm: null,
      averageLatency: null,
    },
    platforms: [],
    models: [],
    trends: [],
    usage: [],
  },
  sources: [],
});

const state = reactive<AppState>(createInitialState());
const isConnecting = ref(false);
const selectedSourceId = ref<string | null>(null);
let events: EventSource | null = null;

const isLoginPending = computed(() => state.status === "collecting" && state.browserUrl.includes("/login"));
const statusCopy = computed(() => {
  if (state.status === "starting") return "正在打开授权浏览器";
  if (state.status === "collecting") return isLoginPending.value ? "等待在授权浏览器中登录" : "正在采集 JSON 响应";
  if (state.status === "error") return "采集器需要处理一个错误";
  return "尚未连接 SevnX";
});

const isLive = computed(() => state.status === "collecting");
const hasData = computed(() => state.sources.length > 0 || state.snapshot.usage.length > 0);
const selectedSource = computed<SourceRecord | null>(() => {
  if (selectedSourceId.value) {
    return state.sources.find((source) => source.id === selectedSourceId.value) || null;
  }
  return state.sources[0] || null;
});

const metricCards = computed(() => {
  const account = state.snapshot.account;
  return [
    { label: "余额", value: formatMoney(account.balance), detail: "可用余额", tone: "green" },
    { label: "今日请求", value: formatCompact(account.requests), detail: "当前采集范围", tone: "blue" },
    { label: "今日消费", value: formatMoney(account.spend, 4), detail: budgetDetail.value, tone: "yellow" },
    { label: "今日 Token", value: formatCompact(account.tokens), detail: tokenDetail.value, tone: "red" },
    { label: "平均响应", value: formatLatency(account.averageLatency), detail: "响应耗时", tone: "neutral" },
  ];
});

const budgetDetail = computed(() => {
  const { spend, spendLimit } = state.snapshot.account;
  if (spend === null || spendLimit === null) return "已识别消费指标";
  return `${formatMoney(spend, 4)} / ${formatMoney(spendLimit, 4)}`;
});

const tokenDetail = computed(() => {
  const { inputTokens, outputTokens } = state.snapshot.account;
  if (inputTokens === null && outputTokens === null) return "输入与输出待识别";
  return `输入 ${formatCompact(inputTokens)} / 输出 ${formatCompact(outputTokens)}`;
});

const budgetRatio = computed(() => {
  const { spend, spendLimit } = state.snapshot.account;
  if (spend === null || spendLimit === null || spendLimit <= 0) return 0;
  return Math.min(100, Math.max(0, (spend / spendLimit) * 100));
});

const maxModelTokens = computed(() => Math.max(1, ...state.snapshot.models.map((row) => row.tokens || 0)));
const maxPlatformTokens = computed(() => Math.max(1, ...state.snapshot.platforms.map((row) => row.tokens || 0)));
const maxTrendTokens = computed(() => Math.max(1, ...state.snapshot.trends.map((row) => row.tokens || 0)));
const recentSources = computed(() => state.sources.slice(0, 12));
const recentUsage = computed(() => state.snapshot.usage.slice(0, 24));
const sourcePreview = computed(() => {
  if (!selectedSource.value) return "暂无接口响应样本";
  return JSON.stringify(selectedSource.value.sample, null, 2);
});

function applyState(next: AppState) {
  Object.assign(state, next);
  if (!selectedSourceId.value && next.sources.length > 0) selectedSourceId.value = next.sources[0].id;
  if (selectedSourceId.value && !next.sources.some((source) => source.id === selectedSourceId.value)) {
    selectedSourceId.value = next.sources[0]?.id || null;
  }
}

async function loadState() {
  try {
    const response = await fetch("/api/state", { cache: "no-store" });
    if (response.ok) applyState(await response.json() as AppState);
  } catch (error) {
    state.lastError = error instanceof Error ? error.message : String(error);
  }
}

async function post(path: string) {
  const response = await fetch(path, { method: "POST" });
  if (!response.ok) throw new Error(`${path} ${response.status}`);
}

async function connect() {
  if (isConnecting.value || isLive.value) return;
  isConnecting.value = true;
  state.lastError = null;
  try {
    await post("/api/connect");
  } catch (error) {
    state.lastError = error instanceof Error ? error.message : String(error);
  } finally {
    isConnecting.value = false;
  }
}

async function disconnect() {
  try {
    await post("/api/disconnect");
  } catch (error) {
    state.lastError = error instanceof Error ? error.message : String(error);
  }
}

async function refresh() {
  try {
    await post("/api/refresh");
  } catch (error) {
    state.lastError = error instanceof Error ? error.message : String(error);
  }
}

async function openLogin() {
  try {
    await post("/api/open-login");
  } catch (error) {
    state.lastError = error instanceof Error ? error.message : String(error);
  }
}

async function resetData() {
  try {
    await post("/api/reset");
  } catch (error) {
    state.lastError = error instanceof Error ? error.message : String(error);
  }
}

function formatNumber(value: number | null, maximumFractionDigits = 0) {
  if (value === null || value === undefined || Number.isNaN(value)) return "--";
  return new Intl.NumberFormat("en-US", { maximumFractionDigits }).format(value);
}

function formatCompact(value: number | null) {
  if (value === null || value === undefined || Number.isNaN(value)) return "--";
  const absolute = Math.abs(value);
  if (absolute >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1)}B`;
  if (absolute >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (absolute >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return formatNumber(value);
}

function formatMoney(value: number | null, digits = 2) {
  if (value === null || value === undefined || Number.isNaN(value)) return "--";
  return `$${value.toFixed(digits)}`;
}

function formatLatency(value: number | null) {
  if (value === null || value === undefined || Number.isNaN(value)) return "--";
  return `${value >= 1000 ? `${(value / 1000).toFixed(2)}s` : `${Math.round(value)}ms`}`;
}

function formatTime(value: string | null) {
  if (!value) return "--";
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  return parsed.toLocaleString("zh-CN", { hour12: false });
}

function text(value: string | number | null | undefined) {
  return value === null || value === undefined || value === "" ? "--" : String(value);
}

function widthFor(value: number | null, max: number) {
  if (value === null || value === undefined || max <= 0) return "0%";
  return `${Math.max(4, Math.min(100, (value / max) * 100))}%`;
}

function sourceKeyList(source: SourceRecord) {
  return source.topLevelKeys.length ? source.topLevelKeys.join(", ") : "未发现顶层字段";
}

onMounted(async () => {
  await loadState();
  events = new EventSource("/api/events");
  events.addEventListener("state", (event) => {
    const message = event as MessageEvent<string>;
    applyState(JSON.parse(message.data) as AppState);
  });
  events.onerror = () => {
    if (state.status === "collecting") state.lastError = "实时连接暂时中断，页面会继续保留最近数据";
  };
});

onBeforeUnmount(() => {
  events?.close();
});
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <a class="brand" href="#top" aria-label="回到总览">
        <span class="brand-mark">SX</span>
        <span>
          <strong>SevnX</strong>
          <small>Local Observatory</small>
        </span>
      </a>

      <nav class="side-nav" aria-label="页面导航">
        <a class="nav-item active" href="#overview"><span class="nav-index">01</span>总览</a>
        <a class="nav-item" href="#distribution"><span class="nav-index">02</span>分布</a>
        <a class="nav-item" href="#usage"><span class="nav-index">03</span>使用明细</a>
        <a class="nav-item" href="#sources"><span class="nav-index">04</span>接口目录</a>
      </nav>

      <div class="sidebar-foot">
        <span class="eyebrow">SESSION</span>
        <p class="session-name">授权浏览器会话</p>
        <p class="session-copy">数据仅在本机采集和展示。</p>
        <span class="status-line" :class="`status-${state.status}`">
          <i class="status-dot" />
          {{ statusCopy }}
        </span>
      </div>
    </aside>

    <main id="top" class="workspace">
      <header class="topbar">
        <div class="breadcrumbs"><span>LOCAL</span><b>/</b><span>SEVNX</span></div>
        <div class="topbar-actions">
          <span class="last-seen">{{ state.updatedAt ? `更新于 ${formatTime(state.updatedAt)}` : "等待数据" }}</span>
          <button class="icon-button" type="button" title="刷新授权页面" :disabled="!isLive" @click="refresh">↻</button>
          <button class="text-button" type="button" @click="isLive ? disconnect() : connect()">
            <span>{{ isLive ? "断开" : "连接" }}</span>
            <span class="button-arrow">{{ isLive ? "×" : "+" }}</span>
          </button>
        </div>
      </header>

      <section id="overview" class="intro-section">
        <div>
          <p class="eyebrow">SEVNX / DATA SURFACE</p>
          <h1>Local <em>observatory</em></h1>
          <p class="intro-copy">从已授权的 SevnX dashboard 会话采集可验证的使用数据，保持字段开放，按实际响应持续扩展。</p>
        </div>
        <div class="connection-box" :class="`connection-${state.status}`">
          <div class="connection-head">
            <span class="status-line"><i class="status-dot" />{{ statusCopy }}</span>
            <span class="connection-code">{{ isLive ? (isLoginPending ? "AUTH" : "LIVE") : state.status === "error" ? "CHECK" : "READY" }}</span>
          </div>
          <p class="connection-url">{{ state.browserUrl }}</p>
          <button v-if="!isLive" class="primary-button" type="button" :disabled="isConnecting" @click="connect">
            <span class="button-mark">+</span>
            {{ isConnecting ? "正在准备" : "打开授权浏览器" }}
          </button>
          <button v-else-if="isLoginPending" class="primary-button" type="button" @click="openLogin">
            <span class="button-mark">↗</span>
            打开登录页
          </button>
          <button v-else class="secondary-button" type="button" @click="refresh">
            <span class="button-mark">↻</span>
            刷新 dashboard
          </button>
        </div>
      </section>

      <p v-if="state.lastError" class="error-banner">{{ state.lastError }}</p>

      <section class="section-block metrics-section">
        <div class="section-heading">
          <div><p class="eyebrow">AT A GLANCE</p><h2>Account pulse</h2></div>
          <span class="section-note">{{ hasData ? `${state.sources.length} 个接口已观察` : "尚无已采集响应" }}</span>
        </div>
        <div class="metric-grid">
          <article v-for="metric in metricCards" :key="metric.label" class="metric-card" :class="`tone-${metric.tone}`">
            <div class="metric-top"><span>{{ metric.label }}</span><i /></div>
            <strong>{{ metric.value }}</strong>
            <small>{{ metric.detail }}</small>
          </article>
        </div>
      </section>

      <section class="signal-grid">
        <article class="data-panel budget-panel">
          <div class="panel-heading"><div><p class="eyebrow">BUDGET</p><h3>消费进度</h3></div><span>{{ budgetRatio ? `${budgetRatio.toFixed(1)}%` : "--" }}</span></div>
          <div class="budget-track"><span :style="{ width: `${budgetRatio}%` }" /></div>
          <div class="panel-meta"><span>{{ formatMoney(state.snapshot.account.spend, 4) }}</span><span>{{ state.snapshot.account.spendLimit !== null ? formatMoney(state.snapshot.account.spendLimit, 4) : "未识别上限" }}</span></div>
        </article>
        <article class="data-panel performance-panel">
          <div class="panel-heading"><div><p class="eyebrow">PERFORMANCE</p><h3>吞吐与响应</h3></div><span class="panel-symbol">/</span></div>
          <div class="performance-grid">
            <div><span>RPM</span><strong>{{ formatCompact(state.snapshot.account.rpm) }}</strong></div>
            <div><span>TPM</span><strong>{{ formatCompact(state.snapshot.account.tpm) }}</strong></div>
            <div><span>平均响应</span><strong>{{ formatLatency(state.snapshot.account.averageLatency) }}</strong></div>
          </div>
        </article>
      </section>

      <section id="distribution" class="section-block">
        <div class="section-heading"><div><p class="eyebrow">DISTRIBUTION</p><h2>平台与模型</h2></div><span class="section-note">按采集到的有效响应合并</span></div>
        <div class="distribution-grid">
          <article class="data-panel">
            <div class="panel-heading"><div><p class="eyebrow">PLATFORM</p><h3>按平台拆分</h3></div><span>{{ state.snapshot.platforms.length }} 个</span></div>
            <div v-if="state.snapshot.platforms.length" class="bar-list">
              <div v-for="row in state.snapshot.platforms" :key="row.label" class="bar-row">
                <div class="bar-label"><span>{{ row.label }}</span><b>{{ formatCompact(row.tokens) }}</b></div>
                <div class="bar-track"><i :style="{ width: widthFor(row.tokens, maxPlatformTokens) }" /></div>
                <div class="bar-meta"><span>{{ formatNumber(row.requests) }} requests</span><span>{{ formatMoney(row.spend, 4) }}</span></div>
              </div>
            </div>
            <div v-else class="empty-line">平台分布将在对应 JSON 响应出现后填充。</div>
          </article>

          <article class="data-panel">
            <div class="panel-heading"><div><p class="eyebrow">MODEL</p><h3>模型分布</h3></div><span>{{ state.snapshot.models.length }} 个</span></div>
            <div v-if="state.snapshot.models.length" class="model-list">
              <div v-for="row in state.snapshot.models" :key="row.label" class="model-row">
                <div class="model-main"><span class="model-dot" /><strong>{{ row.label }}</strong><small>{{ formatNumber(row.requests) }} req</small></div>
                <div class="model-values"><span>{{ formatCompact(row.tokens) }}</span><b>{{ formatMoney(row.spend, 4) }}</b></div>
                <div class="bar-track"><i :style="{ width: widthFor(row.tokens, maxModelTokens) }" /></div>
              </div>
            </div>
            <div v-else class="empty-line">模型分布将在对应 JSON 响应出现后填充。</div>
          </article>
        </div>
      </section>

      <section class="section-block trend-section">
        <div class="section-heading"><div><p class="eyebrow">TREND</p><h2>Token 使用趋势</h2></div><span class="section-note">{{ state.snapshot.trends.length ? `${state.snapshot.trends.length} 个时间点` : "等待趋势数据" }}</span></div>
        <div class="trend-panel data-panel">
          <div v-if="state.snapshot.trends.length" class="trend-chart">
            <div v-for="point in state.snapshot.trends.slice(-24)" :key="point.label" class="trend-item">
              <div class="trend-bar"><i :style="{ height: widthFor(point.tokens, maxTrendTokens) }" /></div>
              <span>{{ point.label }}</span>
            </div>
          </div>
          <div v-else class="trend-empty"><strong>等待可识别的时间序列</strong><span>接口目录会保留原始字段，新增趋势格式不会被丢弃。</span></div>
        </div>
      </section>

      <section id="usage" class="section-block">
        <div class="section-heading"><div><p class="eyebrow">RECENT USAGE</p><h2>最近使用</h2></div><a class="inline-link" href="https://www.sevnx.one/usage" target="_blank" rel="noreferrer">打开原页面 <span>→</span></a></div>
        <div class="table-shell">
          <table>
            <thead><tr><th>时间</th><th>模型</th><th>端点</th><th>Token</th><th>费用</th><th>延迟</th><th>分组</th></tr></thead>
            <tbody>
              <tr v-for="record in recentUsage" :key="record.id">
                <td class="time-cell">{{ formatTime(record.time) }}</td>
                <td><strong>{{ text(record.model) }}</strong><small>{{ text(record.reasoning) }}</small></td>
                <td class="mono-cell">{{ text(record.endpoint) }}</td>
                <td>{{ formatCompact(record.tokens) }}</td>
                <td>{{ formatMoney(record.spend, 4) }}</td>
                <td>{{ formatLatency(record.latency) }}</td>
                <td>{{ text(record.group) }}</td>
              </tr>
              <tr v-if="!recentUsage.length"><td colspan="7" class="empty-table">暂无明细。连接采集器后，符合明细结构的 JSON 响应会出现在这里。</td></tr>
            </tbody>
          </table>
        </div>
      </section>

      <section id="sources" class="section-block sources-section">
        <div class="section-heading"><div><p class="eyebrow">SOURCE CATALOG</p><h2>接口目录</h2></div><div class="source-actions"><span class="section-note">样本已脱敏</span><button class="quiet-button" type="button" :disabled="!state.sources.length" @click="resetData">清空采集</button></div></div>
        <div class="sources-layout">
          <div class="source-list">
            <button v-for="source in recentSources" :key="source.id" class="source-item" :class="{ selected: selectedSource?.id === source.id }" type="button" @click="selectedSourceId = source.id">
              <span class="source-method">{{ source.method }}</span>
              <span class="source-info"><strong>{{ source.path }}</strong><small>{{ sourceKeyList(source) }}</small></span>
              <span class="source-count">{{ source.count }}×</span>
            </button>
            <div v-if="!recentSources.length" class="empty-source">连接后会自动建立接口目录。</div>
          </div>
          <div class="source-detail">
            <div class="source-detail-head"><span class="eyebrow">RESPONSE SAMPLE</span><span v-if="selectedSource">{{ selectedSource.status }} · {{ selectedSource.contentType }}</span></div>
            <pre>{{ sourcePreview }}</pre>
          </div>
        </div>
      </section>

      <footer class="footer-line"><span>SevnX Local Observatory</span><span>本地采集 · {{ isLive ? "LIVE" : "IDLE" }}</span></footer>
    </main>
  </div>
</template>
