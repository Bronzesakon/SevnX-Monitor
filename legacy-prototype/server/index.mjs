import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createServer as createViteServer } from "vite";

const projectRoot = path.resolve(fileURLToPath(new URL("../", import.meta.url)));
const profileDir = path.join(projectRoot, ".sevnx-profile");
const loginUrl = "https://www.sevnx.one/login?redirect=/dashboard";
const edgeCdpPort = Number(process.env.SEVNX_EDGE_CDP_PORT || 9223);
const edgeCdpUrl = `http://127.0.0.1:${edgeCdpPort}`;
const useVisibleEdge = process.env.SEVNX_USE_VISIBLE_EDGE !== "0";
const port = Number(process.env.PORT || 5173);

const emptyAccount = () => ({
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
});

const emptySnapshot = () => ({
  account: emptyAccount(),
  platforms: [],
  models: [],
  trends: [],
  usage: [],
});

const state = {
  status: "idle",
  browserUrl: "https://www.sevnx.one/dashboard",
  sessionStartedAt: null,
  updatedAt: null,
  lastError: null,
  snapshot: emptySnapshot(),
  sources: [],
};

const clients = new Set();
let browserContext = null;
let browserConnection = null;
let activePage = null;
let launchedEdgeProcess = null;
let closingContext = false;

function publicState() {
  return {
    status: state.status,
    browserUrl: state.browserUrl,
    sessionStartedAt: state.sessionStartedAt,
    updatedAt: state.updatedAt,
    lastError: state.lastError,
    snapshot: state.snapshot,
    sources: state.sources,
  };
}

function sendJson(res, statusCode, payload) {
  const body = JSON.stringify(payload);
  res.writeHead(statusCode, {
    "content-type": "application/json; charset=utf-8",
    "cache-control": "no-store",
    "access-control-allow-origin": "*",
  });
  res.end(body);
}

function sendEvent(res, event, payload) {
  res.write(`event: ${event}\ndata: ${JSON.stringify(payload)}\n\n`);
}

function broadcast(event = "state") {
  const payload = publicState();
  for (const client of clients) {
    try {
      sendEvent(client, event, payload);
    } catch {
      clients.delete(client);
    }
  }
}

function markChanged() {
  state.updatedAt = new Date().toISOString();
  broadcast();
}

function isSensitiveKey(key) {
  return /authorization|cookie|password|secret|api[_-]?key|access[_-]?token|refresh[_-]?token|id[_-]?token/i.test(key);
}

function redact(value, key = "", depth = 0) {
  if (isSensitiveKey(key)) return "[redacted]";
  if (depth > 4) return "[depth limited]";
  if (value === null || typeof value === "number" || typeof value === "boolean") return value;
  if (typeof value === "string") return value.length > 500 ? `${value.slice(0, 500)}...` : value;
  if (Array.isArray(value)) return value.slice(0, 40).map((item) => redact(item, key, depth + 1));
  if (typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .slice(0, 100)
        .map(([childKey, childValue]) => [childKey, redact(childValue, childKey, depth + 1)]),
    );
  }
  return String(value);
}

function safeUrl(rawUrl) {
  try {
    const target = new URL(rawUrl);
    return `${target.origin}${target.pathname}`;
  } catch {
    return rawUrl.split("?")[0];
  }
}

function isSevnxLoginUrl(rawUrl) {
  try {
    const target = new URL(rawUrl);
    return target.origin === "https://www.sevnx.one" && target.pathname.startsWith("/login");
  } catch {
    return rawUrl.includes("/login");
  }
}

function hashString(value) {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(16);
}

function normalizedKey(key) {
  return String(key).toLowerCase().replace(/[^a-z0-9]/g, "");
}

function numberValue(value) {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value !== "string") return null;
  const clean = value.replace(/,/g, "").trim();
  const match = clean.match(/-?[0-9]+(?:\.[0-9]+)?/);
  if (!match) return null;
  const number = Number(match[0]);
  if (!Number.isFinite(number)) return null;
  const suffix = clean.slice(match.index + match[0].length).trim().toLowerCase()[0];
  if (suffix === "k") return number * 1_000;
  if (suffix === "m") return number * 1_000_000;
  if (suffix === "b") return number * 1_000_000_000;
  return number;
}

function textValue(value) {
  if (value === null || value === undefined) return null;
  if (typeof value === "string") return value.trim() || null;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return null;
}

function leafEntries(value, pathParts = [], output = [], depth = 0) {
  if (output.length > 5000 || depth > 6 || value === null || value === undefined) return output;
  if (Array.isArray(value)) {
    value.slice(0, 160).forEach((item, index) => leafEntries(item, [...pathParts, String(index)], output, depth + 1));
    return output;
  }
  if (typeof value !== "object") {
    const key = pathParts[pathParts.length - 1] || "value";
    output.push({ key, normalized: normalizedKey(key), path: pathParts.join("."), value });
    return output;
  }
  for (const [key, child] of Object.entries(value)) {
    leafEntries(child, [...pathParts, key], output, depth + 1);
  }
  return output;
}

function objectNodes(value, output = [], depth = 0) {
  if (output.length > 2000 || depth > 5 || value === null || typeof value !== "object") return output;
  if (Array.isArray(value)) {
    value.slice(0, 160).forEach((item) => objectNodes(item, output, depth + 1));
    return output;
  }
  output.push(value);
  for (const child of Object.values(value)) objectNodes(child, output, depth + 1);
  return output;
}

function matchesAlias(key, aliases) {
  const value = normalizedKey(key);
  return aliases.some((alias) => value === alias || value.endsWith(alias) || value.includes(alias));
}

function findMetric(entries, aliases) {
  const match = entries.find((entry) => matchesAlias(entry.normalized, aliases) && numberValue(entry.value) !== null);
  return match ? numberValue(match.value) : null;
}

function directField(object, aliases, mode = "number") {
  if (!object || typeof object !== "object") return null;
  for (const [key, value] of Object.entries(object)) {
    if (!matchesAlias(key, aliases)) continue;
    const result = mode === "text" ? textValue(value) : numberValue(value);
    if (result !== null) return result;
  }
  return null;
}

function directText(object, aliases) {
  return directField(object, aliases, "text");
}

function rowFromObject(object, labelAliases) {
  const label = directText(object, labelAliases);
  if (!label) return null;
  const requests = directField(object, ["requests", "requestcount", "count", "calls"]);
  const tokens = directField(object, ["tokens", "totaltokens", "tokencount", "usage"]);
  const spend = directField(object, ["actualcost", "realcost", "spend", "cost", "charge", "amount"]);
  const standardSpend = directField(object, ["standardcost", "originalcost", "listcost"]);
  if (requests === null && tokens === null && spend === null && standardSpend === null) return null;
  return { label, requests, tokens, spend, standardSpend };
}

function uniqueRows(rows) {
  const byLabel = new Map();
  for (const row of rows) {
    if (!row?.label) continue;
    const previous = byLabel.get(row.label);
    byLabel.set(row.label, previous ? {
      label: row.label,
      requests: row.requests ?? previous.requests,
      tokens: row.tokens ?? previous.tokens,
      spend: row.spend ?? previous.spend,
      standardSpend: row.standardSpend ?? previous.standardSpend,
    } : row);
  }
  return [...byLabel.values()].slice(0, 40);
}

function usageFromObject(object, index) {
  const model = directText(object, ["model", "modelname"]);
  const endpoint = directText(object, ["endpoint", "path", "route"]);
  const time = directText(object, ["time", "timestamp", "createdat", "created_at", "date"]);
  if (!model && !endpoint && !time) return null;
  const tokens = directField(object, ["tokens", "totaltokens", "tokenusage"]);
  const spend = directField(object, ["actualcost", "realcost", "spend", "cost", "charge", "amount"]);
  if (tokens === null && spend === null && directField(object, ["latency", "duration", "responseduration"]) === null) return null;
  return {
    id: `${index}-${hashString(JSON.stringify(object).slice(0, 500))}`,
    apiKey: directText(object, ["apikey", "keyname", "key"]),
    model,
    reasoning: directText(object, ["reasoning", "reasoningeffort", "effort"]),
    endpoint,
    ip: directText(object, ["ip", "ipaddress"]),
    group: directText(object, ["group", "groupname"]),
    type: directText(object, ["type", "requesttype"]),
    billingMode: directText(object, ["billingmode", "billingtype", "billing"]),
    tokens,
    inputTokens: directField(object, ["inputtokens", "prompttokens", "input"]),
    outputTokens: directField(object, ["outputtokens", "responsetokens", "completiontokens", "output"]),
    cacheTokens: directField(object, ["cachetokens", "cachedtokens", "cache"]),
    spend,
    standardSpend: directField(object, ["standardcost", "originalcost", "listcost"]),
    latency: directField(object, ["latency", "duration", "responseduration", "responsetime"]),
    time,
  };
}

function trendFromObject(object) {
  const label = directText(object, ["date", "day", "hour", "period", "label", "time"]);
  if (!label) return null;
  const requests = directField(object, ["requests", "requestcount", "calls"]);
  const tokens = directField(object, ["tokens", "totaltokens", "usage"]);
  const spend = directField(object, ["actualcost", "realcost", "spend", "cost", "charge"]);
  if (requests === null && tokens === null && spend === null) return null;
  return { label, requests, tokens, spend };
}

function normalizePayload(payload) {
  const entries = leafEntries(payload);
  const objects = objectNodes(payload);
  const account = {
    balance: findMetric(entries, ["availablebalance", "totalbalance", "walletbalance", "accountbalance", "balance"]),
    apiKeys: findMetric(entries, ["apikeycount", "apikeys", "totalkeys", "keycount"]),
    enabledKeys: findMetric(entries, ["enabledkeys", "activekeys", "enabledkeycount", "activekeycount"]),
    requests: findMetric(entries, ["todayrequests", "totalrequests", "requestcount", "requests"]),
    spend: findMetric(entries, ["todayspend", "todaycost", "actualcost", "totalcost", "spend", "cost", "charge"]),
    spendLimit: findMetric(entries, ["dailyspendlimit", "spendlimit", "costlimit", "budgetlimit"]),
    tokens: findMetric(entries, ["todaytokens", "totaltokens", "tokencount", "tokens"]),
    inputTokens: findMetric(entries, ["inputtokens", "prompttokens"]),
    outputTokens: findMetric(entries, ["outputtokens", "responsetokens", "completiontokens"]),
    cachedTokens: findMetric(entries, ["cachedtokens", "cachetokens"]),
    rpm: findMetric(entries, ["rpm", "requestsperminute"]),
    tpm: findMetric(entries, ["tpm", "tokensperminute"]),
    averageLatency: findMetric(entries, ["averagelatency", "meanlatency", "avglatency"]),
  };

  const models = uniqueRows(objects.map((object) => rowFromObject(object, ["model", "modelname"])));
  const platforms = uniqueRows(objects.map((object) => rowFromObject(object, ["platform", "provider", "source", "channel"])));
  const trends = objects.map(trendFromObject).filter(Boolean).slice(0, 80);
  const usage = objects.map(usageFromObject).filter(Boolean).slice(0, 120);

  return { account, models, platforms, trends, usage };
}

function mergeRows(previous, incoming) {
  const merged = new Map(previous.map((row) => [row.label, row]));
  for (const row of incoming) {
    const old = merged.get(row.label);
    merged.set(row.label, old ? {
      ...old,
      requests: row.requests ?? old.requests,
      tokens: row.tokens ?? old.tokens,
      spend: row.spend ?? old.spend,
      standardSpend: row.standardSpend ?? old.standardSpend,
    } : row);
  }
  return [...merged.values()].slice(0, 80);
}

function mergeSnapshot(previous, incoming) {
  const account = { ...previous.account };
  for (const [key, value] of Object.entries(incoming.account)) {
    if (value !== null && value !== undefined) account[key] = value;
  }
  return {
    account,
    platforms: mergeRows(previous.platforms, incoming.platforms),
    models: mergeRows(previous.models, incoming.models),
    trends: mergeRows(previous.trends, incoming.trends),
    usage: [...incoming.usage, ...previous.usage.filter((row) => !incoming.usage.some((item) => item.id === row.id))].slice(0, 160),
  };
}

async function inspectResponse(response) {
  const request = response.request();
  const rawUrl = response.url();
  const headers = response.headers();
  const contentType = headers["content-type"] || "";
  const looksRelevant = /json|api|dashboard|usage|stats|billing|account|summary/i.test(`${contentType} ${rawUrl}`);
  if (!looksRelevant) return;

  let body;
  try {
    body = await response.text();
  } catch {
    return;
  }
  if (!body || body.length > 3_000_000) return;

  let payload;
  try {
    payload = JSON.parse(body);
  } catch {
    return;
  }

  const pathValue = safeUrl(rawUrl);
  const id = hashString(`${request.method()} ${pathValue}`);
  const existing = state.sources.find((source) => source.id === id);
  const source = {
    id,
    path: pathValue,
    method: request.method(),
    status: response.status(),
    contentType: contentType.split(";")[0] || "application/json",
    bytes: Buffer.byteLength(body),
    topLevelKeys: Array.isArray(payload) ? ["[array]"] : Object.keys(payload || {}).slice(0, 80),
    seenAt: new Date().toISOString(),
    count: (existing?.count || 0) + 1,
    sample: redact(payload),
  };
  if (existing) Object.assign(existing, source);
  else state.sources.unshift(source);
  state.sources = state.sources.slice(0, 80);
  state.snapshot = mergeSnapshot(state.snapshot, normalizePayload(payload));
  markChanged();
}

function attachPage(page) {
  activePage = page;
  state.browserUrl = page.url() || state.browserUrl;
  page.on("response", (response) => {
    void inspectResponse(response);
  });
  page.on("framenavigated", (frame) => {
    if (frame === page.mainFrame()) {
      state.browserUrl = frame.url();
      markChanged();
    }
  });
}

function findInstalledBrowser() {
  const candidates = [
    process.env.SEVNX_BROWSER_EXECUTABLE_PATH,
    "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
  ].filter(Boolean);
  return candidates.find((candidate) => existsSync(candidate)) || null;
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function connectToVisibleEdge(chromium, executablePath) {
  try {
    return await chromium.connectOverCDP(edgeCdpUrl);
  } catch {}

  const child = spawn(
    executablePath,
    [
      `--remote-debugging-port=${edgeCdpPort}`,
      `--user-data-dir=${profileDir}`,
      "--profile-directory=Default",
      "--new-window",
      "--start-maximized",
      "--no-first-run",
      "--no-default-browser-check",
      "--remote-allow-origins=*",
      loginUrl,
    ],
    { detached: true, stdio: "ignore", windowsHide: false },
  );
  child.on("error", () => {});
  child.unref();
  launchedEdgeProcess = child;

  let lastError = null;
  for (let attempt = 0; attempt < 80; attempt += 1) {
    try {
      return await chromium.connectOverCDP(edgeCdpUrl);
    } catch (error) {
      lastError = error;
      await delay(250);
    }
  }
  throw lastError || new Error(`无法连接到 Edge 调试端口 ${edgeCdpUrl}`);
}

function handleBrowserClosed() {
  browserContext = null;
  browserConnection = null;
  activePage = null;
  if (!closingContext) {
    state.status = "idle";
    broadcast();
  }
  closingContext = false;
}

async function startCollector() {
  if (browserContext) {
    if (activePage) await activePage.bringToFront().catch(() => {});
    return;
  }

  state.status = "starting";
  state.lastError = null;
  state.sessionStartedAt = new Date().toISOString();
  broadcast();

  try {
    const { chromium } = await import("playwright");
    const executablePath = findInstalledBrowser();
    if (!executablePath) throw new Error("未找到 Edge 或 Chrome 可执行文件");

    await mkdir(profileDir, { recursive: true });
    if (useVisibleEdge) {
      browserConnection = await connectToVisibleEdge(chromium, executablePath);
      browserContext = browserConnection.contexts()[0] || null;
      if (!browserContext) throw new Error("Edge 未提供可用的浏览器上下文");
    } else {
      const options = {
        headless: false,
        viewport: { width: 1440, height: 960 },
      };
      if (process.env.SEVNX_BROWSER_CHANNEL) options.channel = process.env.SEVNX_BROWSER_CHANNEL;
      if (!options.channel) options.executablePath = executablePath;
      browserContext = await chromium.launchPersistentContext(profileDir, options);
    }

    state.status = "collecting";
    state.lastError = null;
    const pages = browserContext.pages();
    activePage = pages.find((page) => page.url().includes("sevnx.one")) || pages[0] || await browserContext.newPage();
    attachPage(activePage);
    if (!activePage.url().includes("sevnx.one")) {
      try {
        await activePage.goto("https://www.sevnx.one/dashboard", { waitUntil: "domcontentloaded", timeout: 45_000 });
      } catch (error) {
        if (!isSevnxLoginUrl(activePage.url())) throw error;
      }
    } else if (!isSevnxLoginUrl(activePage.url())) {
      await activePage.reload({ waitUntil: "domcontentloaded", timeout: 45_000 }).catch(() => {});
    }
    markChanged();
    if (browserConnection) browserConnection.on("disconnected", handleBrowserClosed);
    browserContext.on("close", handleBrowserClosed);
  } catch (error) {
    browserConnection?.disconnect();
    browserContext = null;
    browserConnection = null;
    activePage = null;
    state.status = "error";
    state.lastError = error instanceof Error ? error.message : String(error);
    broadcast();
  }
}

async function stopCollector() {
  if (!browserContext && !browserConnection) return;
  closingContext = true;
  if (browserConnection) browserConnection.disconnect();
  else await browserContext?.close().catch(() => {});
  browserContext = null;
  browserConnection = null;
  activePage = null;
  launchedEdgeProcess = null;
  state.status = "idle";
  state.lastError = null;
  broadcast();
}

async function openLoginPage() {
  if (!browserContext) await startCollector();
  if (!activePage) return false;

  await activePage.bringToFront().catch(() => {});
  if (activePage.url().includes("/login")) {
    state.lastError = null;
    markChanged();
    return true;
  }

  try {
    await activePage.goto(loginUrl, { waitUntil: "domcontentloaded", timeout: 45_000 });
  } catch (error) {
    if (activePage.url().includes("/login")) state.lastError = null;
    else state.lastError = error instanceof Error ? error.message : String(error);
  }
  markChanged();
  return true;
}

function readRequestBody(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    req.on("error", () => resolve(""));
  });
}

async function handleApi(req, res) {
  const url = new URL(req.url || "/", "http://127.0.0.1");
  if (req.method === "GET" && url.pathname === "/api/state") return sendJson(res, 200, publicState());
  if (req.method === "GET" && url.pathname === "/api/health") return sendJson(res, 200, { ok: true });
  if (req.method === "GET" && url.pathname === "/api/events") {
    res.writeHead(200, {
      "content-type": "text/event-stream; charset=utf-8",
      "cache-control": "no-cache",
      connection: "keep-alive",
      "access-control-allow-origin": "*",
    });
    clients.add(res);
    sendEvent(res, "state", publicState());
    req.on("close", () => clients.delete(res));
    return;
  }
  if (req.method === "POST" && url.pathname === "/api/connect") {
    void startCollector();
    return sendJson(res, 202, { accepted: true });
  }
  if (req.method === "POST" && url.pathname === "/api/disconnect") {
    await stopCollector();
    return sendJson(res, 200, { accepted: true });
  }
  if (req.method === "POST" && url.pathname === "/api/refresh") {
    await activePage?.reload({ waitUntil: "domcontentloaded", timeout: 45_000 }).catch(() => {});
    return sendJson(res, 200, { accepted: true });
  }
  if (req.method === "POST" && url.pathname === "/api/open-login") {
    const opened = await openLoginPage();
    if (!opened) return sendJson(res, 503, { error: state.lastError || "browser_unavailable" });
    return sendJson(res, 200, { accepted: true });
  }
  if (req.method === "POST" && url.pathname === "/api/reset") {
    await readRequestBody(req);
    state.snapshot = emptySnapshot();
    state.sources = [];
    state.lastError = null;
    markChanged();
    return sendJson(res, 200, { accepted: true });
  }
  return sendJson(res, 404, { error: "not_found" });
}

const vite = await createViteServer({
  root: projectRoot,
  server: {
    middlewareMode: true,
    watch: {
      ignored: [profileDir],
    },
  },
  appType: "spa",
});

const server = createServer(async (req, res) => {
  if ((req.url || "").startsWith("/api/")) return handleApi(req, res);
  vite.middlewares(req, res, () => {
    if (!res.writableEnded) {
      res.statusCode = 404;
      res.end("Not found");
    }
  });
});

server.listen(port, "127.0.0.1", () => {
  console.log(`SevnX Local Observatory: http://127.0.0.1:${port}`);
});

async function shutdown() {
  await stopCollector();
  await vite.close();
  server.close();
}

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
