# SevnX → Codex 顶部注入「余额 / 今日消费 / 今日 Token」横条 · 设计方案

> 本文件汇总结论：向 Codex 桌面应用顶部注入一个显示 SevnX 账户数据的界面（横条 + 小窗），
> 供跨对话的详细工程记忆与后续实现参考。原则：**最大复用参考项目 Codex++ + 最小代码精简实现**。
>
> 参考项目：`e:\sevnX\codex-plusplus`（BigPizzaV3/CodexPlusPlus）。

---

## 1. 背景与目标

- SevnX Monitor（`e:\sevnX`，Tauri v2 + Vue）已能监控 SevnX 账户的 Token 用量、余额等数据。
- 目标：在 Codex 顶部注入一个按钮/横条，显示 **余额、今日消费、今日 Token** 三个量；
  点击后打开小窗（数据展示 + 交互），并支持未登录时点击直接打开登录窗口。
- 关键限定：**Codex 必须带 `--remote-debugging-port` 启动才能被外部注入**；
  由 SevnX 负责启动 Codex（或附加已带调试端口的实例）。

### 三个量（已确认，与现有 BarView 完全一致）

| 标签 | 字段 |
| --- | --- |
| 余额 | `dashboard.balance.display` / `.value` |
| 今日消费 | `dashboard.todaySpend.displayActual` / `.actual` |
| 今日 Token | `dashboard.todayTokens.displayTotal` / `.total` |

- 格式化逻辑内联实现 `moneyDisplay` / `compactTokenValue`（参考 `frontend/src/lib/format.ts`）。
- 数据挂在白名单化的 `AppSnapshot`（`src-tauri/src/model/state.rs`），天然不泄露凭证。

---

## 2. 架构总览（目标数据流）

```
外部 SevnX API ──> SevnxApiClient ──> AppState(AppSnapshot)
                                            │
                         ┌──────────────────┴──────────────────┐
                         ▼                                     ▼
                  自家 WebView(BarView)       新增: 本地HTTP暴露层 ──> 注入Codex的JS
                                                        │
                                                        └─ 动作端点(登录等)
```

- 注入 Codex 的 JS 无法走 Tauri IPC，只能通过 HTTP 访问本地数据。
- **数据层完全复用现成 `AppSnapshot`**，只新增一个本地 HTTP 出口 + 注入脚本。

---

## 3. 后端流程（最大复用 Codex++ + 最小精简）

SevnX 本身是常驻 Tauri 应用，**同时充当 Codex++ 的 launcher + helper**，无需独立 launcher 进程。

```
[桌面快捷方式] ──> sevnx-monitor.exe --launch-codex
                        │
      AppServices 启动（single_instance 插件聚焦已有实例并转发 argv）
        ├─ 1. 起本地HTTP server (127.0.0.1:0)，端口+token写入AppPaths
        ├─ 2. 恢复Session + auto_refresh 开始拉数据
        └─ 3. 处理 --launch-codex 参数 ──> launch_and_inject()
                    │
                    └─ 探测codex路径(复用 app_paths.rs)
                       spawn(codex.exe --remote-debugging-port=9229)
                       轮询 cdp.list_targets → 选中codex主页面target
                       bridge.evaluate → 注入 overlay.js（fetch本地HTTP）
```

### 复用 Codex++ 的模块（文件级搬运）

| Codex++ 模块 | 用途 | 处理 |
| --- | --- | --- |
| `crates/codex-plus-core/src/cdp.rs` | 列 target、选 codex 主页面、loopback+端口强校验 | 整份搬 |
| `crates/codex-plus-core/src/bridge.rs` | CDP WebSocket `Runtime.evaluate` 注入 | 搬 |
| `crates/codex-plus-core/src/app_paths.rs` | 探测 Codex 安装路径（MS Store / 独立安装） | 搬 |
| `crates/codex-plus-core/src/launcher.rs` | `retry_injection`/`try_inject` 骨架、`build_codex_arguments` | 精简 |
| `crates/codex-plus-core/src/windows_integration.rs` | `create_shortcut`（IShellLinkW 建 .lnk） | 搬 |

### 必须砍掉的（Codex++ 膨胀面）

helper 路由集市（delete/export/plugin/relay/proxy/computer_use_guard/dream_skin/user_scripts/zed_remote）全部不要。
SevnX 本地 HTTP 服务只保留 **2 个端点**：

- `GET /sevnx/overlay` → 返回三个量 + auth + 时间戳（白名单）。
- `POST /sevnx/action` → `{"action":"open-login"}` 等动作。

---

## 4. 本地 HTTP 暴露层（Rust 侧新增）

- 新增模块 `services/overlay_server.rs`（或 `codex_inject/`），在 `lib.rs::run()` 的 `setup` 启动。
- **依赖**：当前 `Cargo.toml` 只有 `reqwest`（客户端），需新增 HTTP server 依赖。
  推荐 `axum`（与 tokio 契合）；或轻量 `tiny_http`（单 GET/POST 够用）。
- **端口**：`127.0.0.1:0`（系统分配随机端口），实际端口 + 随机 token 写入 `AppPaths` metadata 文件。
- **端点响应示例**：
  ```json
  {
    "auth": "authenticated",
    "balance":   { "value": "100.5", "display": "100.5" },
    "todaySpend": { "value": "2.25",  "display": "2.25" },
    "todayTokens": { "value": "2000", "display": "2000" },
    "fetchedAt": "2026-08-07T10:00:00Z"
  }
  ```
- **安全**：
  - 只 bind `127.0.0.1`。
  - 随机端口 + 一次性随机 token，注入 JS 请求带 `Authorization: Bearer <token>`，服务端校验。
  - 只出三个量 + auth + 时间，**不回传 `usage` 全量 / 任何凭证**（沿用 `must-not-leave-ipc` 白名单约定）。
  - 返回 `Access-Control-Allow-Origin: *`（否则注入 JS 的 fetch 会被 CSP 拦截）。

---

## 5. 注入 JS + 界面（精简版 Codex++ renderer-inject.js）

1. **入口**：通过 CDP `Runtime.evaluate` 注入到 Codex 主渲染进程。
2. **界面**：复用 `installCodexPlusMenu` / `findNativeMenuInsertionPoint` / `configureCodexPlusTrigger` 定位右上角菜单栏，
   插入拉宽按钮/横条，点击打开小窗；注入脚本通过 CDP 注入参数拿到端口 + token。
3. **数据**：`fetch('http://127.0.0.1:<port>/sevnx/overlay')`，按 `fetchedAt` 判断是否变化，
   轮询间隔与 `auto_refresh` 对齐（可见 30s）。
4. **样式参考**：`.codex-plus-menu-floating`——`position:fixed` + 高 `z-index` + `-webkit-app-region: no-drag`。

---

## 6. 界面风格与 Codex 原生一致（已确认重点）

注入 JS 运行在 Codex 页面内，**可直接访问 Codex 全局样式表**，天然可做到原生一致。

- Codex 样式体系 = OpenAI **token 设计令牌**：
  - CSS 变量：`--token-text-secondary`、`--token-bg-*`、`--token-border` 等。
  - Utility 类：`bg-token-*`、`text-token-*`、`border-token-*`、`h-token-*`
    （参考 Codex++ `headerIconTextButtonClass` / `headerContextButtonClass` 复用的类名串）。
  - 深浅色切换：`html.light` / `html:not(.light)` 选择器。
- **做法（按推荐度）**：
  1. 直接复用 Codex 的 utility 类（`rounded-lg`、`border-token-border`、`text-token-text-secondary`、hover 态等）。
  2. 深浅色用 `var(--token-*)` 或 `html.light/:not(.light)` 跟随，**不要硬编码色值**
     （注意：Codex++ 模态框 `background:#2b2b2b` 是硬编码深色反例，应避免）。
  3. 对齐 Codex 面板的字体（`system-ui`）、圆角（16-18px）、hover/focus、滚动条、间距节奏。
- 图标用内联 SVG + Codex 现有图标风格，不引入外部图标库；保持 Codex 克制扁平风格，不做毛玻璃等 fancy 效果。

---

## 7. Cookie 失效自动续期

现状（`src-tauri/src/auth/session.rs`）已有**被动续期**：
- `apply_set_cookie_headers`（L297）：每次成功响应把服务器 Set-Cookie 合并进 session 并更新 `expires_at`。
- `request_credentials`（L192）：请求前检查过期，逾期才 `mark_expired`（需重登）。
- `auto_refresh` 每 30s 请求 dashboard，顺带触发 Set-Cookie 续期。
- 结论：只要 `auto_refresh` 在跑且服务器下发续期 cookie，即自动续期，无需用户介入。

增强方向：
1. **主动预热续期（推荐，改动小）**：当 `expires_at - now < 阈值`（如 5 分钟）时，
   主动调一次轻量读端点（`dashboard/stats` 或 `/auth/me`）触发 Set-Cookie 续期，抢在到期前。
2. **Refresh Token**：当前只存 cookie + bearer，无独立 refresh_token；若后端支持则接入（需后端配合）。
3. **边界**：服务器 revoke 或短期 cookie 且普通请求不续期时，最终仍需重登。

---

## 8. 小窗内容建议（已确认）

按优先级：
- **顶部**：账户状态（已登录/未登录/已失效）+ 余额。
- **三个主量**（与横条一致，放大）：余额、今日消费、今日 Token。
- **次要**：今日请求数 `todayRequests`、累计 Token `cumulativeTokens`、
  今日输入/输出 Token `todayTokens.input/output`、RPM/TPM `performance`。
- **尾部**：最近更新时间（`fetchedAt` 相对时间）+「刷新」按钮。
- **不显示**：API keys、用户 ID、任何凭证。

---

## 9. 快捷方式创建 + 绑定启动 SevnX

参照 Codex++ `create_shortcut`（`IShellLinkW`，支持 SetPath/SetArguments/SetWorkingDirectory/SetIconLocation）：
1. 建桌面快捷方式：目标 = `sevnx-monitor.exe`，参数 = `--launch-codex`。
2. `lib.rs` 的 `single_instance` 插件（L25-27）已会聚焦已有实例并转发 argv；
   在回调识别 `--launch-codex` → 触发 `services.launch_and_inject()`。
3. 效果：**点快捷方式 → SevnX 常驻/聚焦 → Codex 带调试端口启动 → 注入**。

---

## 10. 未登录状态同步到横条

- 一份 `AppSnapshot` 同时承载数据与 `auth`，天然同步。
- overlay 端点 payload 带 `auth`；`auth != authenticated` 时横条显示「未登录 / 登录失效 / 验证中」+ 占位，隐藏三个量。

---

## 11. 点击横条 → 打开登录窗口的通信

1. 复用本地 HTTP 服务，新增动作端点 `POST /sevnx/action`，body `{"action":"open-login"}`，带同一 `Bearer <token>`。
2. 注入 JS：横条未登录态渲染为可点击，`fetch('http://127.0.0.1:<port>/sevnx/action', …)`。
3. SevnX 侧收到动作：
   - 若要"打开浏览器登录窗口" → 复用 `open_dashboard_in_browser`（`{access_url}/login?redirect=/dashboard`）。
   - 若要登录后自动回填 Session → 复用 `open_login_window`（`src-tauri/src/auth/webview_login.rs`，应用内 WebView2 登录窗）。

---

## 12. 补充未讨论点（健壮性 / 安全 / 体验 / 生命周期）

### 架构与健壮性
1. **多窗口/target**：Codex 可能有主窗口 + quick-chat / avatar-overlay 页面（参考 Codex++ `cdp.rs`），只注入主界面，防重复注入。
2. **重注入 watchdog**：Codex 刷新/路由切换/重启后注入丢失，需周期检查 bridge 健康并重注入（复用 Codex++ `bridge_health_ok` + `start_bridge_watchdog` 骨架）。
3. **注入幂等去重**：类似 Codex++ `removeDuplicateCodexPlusMenus`，防按钮重复注入。
4. **选择器脆弱性**：Codex 升级可能改 DOM，`findNativeMenuInsertionPoint` 选择器会失效，需 floating 兜底 + 预留更新点。
5. **端口冲突 / 附加已运行实例**：调试端口被占用时的回退；Codex 已运行但未带调试端口时的处理。

### 安全与合规
6. **CORS/CSP**：本地 server 返回 `Access-Control-Allow-Origin: *`，否则注入 JS 的 fetch 被 CSP 拦截。
7. **token 管理**：随机 token 安全生成、随启动轮换、只 bind loopback、绝不写日志。
8. **白名单 payload**：overlay 只出三个量 + auth + 时间戳，不泄露 `api_keys`、user ID 等。

### 体验与降级
9. **刷新频率**：注入 JS 轮询与 `auto_refresh` 对齐，用 `fetchedAt` 判断变化，避免空转。
10. **错误态展示**：auth 失效 / 网络失败 / 数据为 null 时分别显示（`--`、stale 时间戳、重试）。
11. **未安装 Codex / 找不到 app_dir**：快捷方式点击后的引导提示。

### 生命周期与分发
12. **卸载/清理**：移除快捷方式、停止注入、清理 metadata 文件。
13. **打包安装**：参考 Codex++ `CodexPlusPlus.nsi` 集成快捷方式与启动项。

### 测试
14. **单元测试**：cdp target 解析、cookie 续期逻辑、overlay payload 白名单（断言不含敏感字段）。
15. **手动验证矩阵**：深浅色切换、多窗口、Codex 刷新后重注入、登录/登出状态切换。

> 其中第 1、6、7 条（多 target、CORS、token 安全）是最易踩坑、建议优先设计。

---

## 13. 落地顺序（分两步，各自可独立验证）

1. **第一步：本地 HTTP 暴露层**（不动注入）——新增 server + 端点 + 安全，
   用 `curl http://127.0.0.1:<port>/sevnx/overlay` 验证数据正确且不泄露敏感字段。
2. **第二步：注入脚本 + 数据联通 + 界面**——CDP 注入、按钮/横条、轮询渲染、登录动作、重注入 watchdog。

---

## 14. 风险与边界

- **未带调试端口的 Codex 无法注入**：需由 SevnX 负责启动 Codex（带 `--remote-debugging-port`）或附加已有实例。
- **CSP/网络**：Codex 渲染进程对 `127.0.0.1` 的 fetch 可行（Codex++ helper 模式已验证）。
- **数据语义**：注入的是「余额/消费/用量」，不是"配额余量"；SevnX 后端当前无配额字段，日后可扩展端点。