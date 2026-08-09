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

### 数据推送通道（可选，加入方案 · 已定稿）
目标：把显示延迟从"最多一个轮询周期(30s)"降到"SevnX 刷新后准实时到达"。

| 方案 | 说明 | 选中 |
| --- | --- | --- |
| **long-poll** | 注入 JS 循环发 `GET /sevnx/overlay/poll?rev=<lastRev>`，SevnX 有新数据立即返回，无则**挂起**到超时(30s)再让 JS 重发。普通 HTTP 即可，实现最简单 | ✅ 推荐 |
| SSE | `GET /sevnx/overlay/stream`，`EventSource` 订阅，SevnX 推送。需连接保持与重连语义 | 次选 |

- **服务端（Rust）**：`AppState` 增加**数据版本 `rev`**，每次 `replace_success` 递增（`last_success_at` 可作为 rev 的替代）。`/poll` 校验 token 后，若当前 `rev > 请求的 rev` 立即返回新 payload；否则经 `tokio watch`/`Notify` 等待，直到新数据或 30s 超时。
- **注入 JS**：收到新 payload 即渲染；网络错误 / 超时 → 短退避后重发（与断联判定共用"连续失败"逻辑，见 §13）。
- **与轮询互斥**：优先走 long-poll；SevnX 断联（fetch 持续失败）时注入 JS 自动降级为轮询并置「已断联」。
- **安全**：`/poll` 与 `/overlay` 同一 token、同一 CORS 约束（见 §12）。

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

## 7. 登录态与自动续期（已实测确认）

### 登录态本质
- 登录接口：`POST {access_url}/api/v1/auth/login`，body `{"email","password"}`，`Content-Type: application/json`，**无 CSRF/无特殊 header**（平行登录极易做）。
- 响应 `data`：`access_token`（JWT）、`refresh_token`（`rt_...`）、`expires_in: 86400`（24h）、`token_type: Bearer`、`user`（含 `balance` 等）。
- **登录态是 Bearer JWT，不是 cookie**。存 `authorization_header = "Bearer <access_token>"` 即可（`PersistedSession` 已有该字段）。

### 自动续期：refresh_token 优先（最优，无需存密码）
- 实测 `POST {access_url}/api/v1/auth/refresh`，body `{"refresh_token":"rt_..."}`，**可用**。
- 返回新的 `access_token` + **新的 `refresh_token`（轮换）** + `expires_in: 86400`。
- 结论：**每次刷新必须用返回的新 `refresh_token` 覆盖旧值存储**（轮换语义）。
- 注：搭建者称"不能刷新"，但实测端点可用——是网页端未暴露，后端实际支持。以实测为准。

### 方案
1. `PersistedSession` 增加 `refresh_token` 字段（DPAPI 加密存储）。
2. 到期前（`expires_at - now < 阈值`，如剩 10-30 分钟）调 `/auth/refresh` 换新，
   更新 `access_token` + `refresh_token` + `expires_at`。
3. 触发点放在 `AppServices::refresh_current` 开头（覆盖手动/自动刷新）。
4. 回退：refresh 失败 → 若存了账密则调 `/auth/login` 重登；否则转手动登录（现有 WebView2 流程）。

### 边界
- 服务器 revoke refresh_token 或 refresh 连续失败时，回退账密重登或手动登录。

---

## 8. 小窗内容建议（已确认）

- **载体（已定稿）**：小窗 = **注入在 Codex 页内的 DOM 浮层**（与横条同源），不是 SevnX 自有 WebView 窗口。
  深色令牌样式天然一致，交互（刷新 / 打开登录）走本地 HTTP action 委托 SevnX，符合整套注入架构。

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

## 12. token 生命周期与验证方案（已定稿）

### 威胁模型（token 防谁）
- token 是服务端对"请求来自被注入 JS"的**共享密钥校验**，真正防的是**浏览器里的其他网页**，不防本机管理员级进程。
- 跨站 `fetch('http://127.0.0.1:PORT/...')` 发不起自定义 header（simple request 限制），且带 token 会触发 CORS 预检被拦。
- 本机恶意进程（同用户）不被浏览器限制，能抓包/读内存拿到 token——真正的防线是**数据低敏**（overlay 只出三个量 + auth + 时间戳）+ 随机端口 + 不落日志。

### 设计要点
| 项 | 设计 |
| --- | --- |
| 身份 | 随机 token（32 字节 crypto），`Authorization: Bearer <token>` 校验 |
| 生命周期 | 绑定 SevnX 进程。每次启动生成新 token，SevnX 退出即失效（server 停，token 无名） |
| 存储 | **只在 SevnX 进程内存，不落盘**。校验端在进程内，重启后旧 token 无意义，无需持久化 |
| 下发 | 通过 CDP 注入脚本时，把 `{overlayPort, token}` 作为注入参数传给横条 JS（浏览器 sandbox 读不了本地文件，注入是唯一可靠通道） |
| CORS | token 在 header，`Access-Control-Allow-Origin: *` 安全（跨站不带 token → 401）。**此约束与 token 强耦合**：一旦去掉 token 或改 Origin 白名单，必须同步收紧 CORS |

### metadata 只存端口，不存 token
- `overlayPort`：SevnX 自己的本地 HTTP 随机端口。
- `debugPort`：Codex 的 CDP 调试端口（快捷方式固定，如 9229）。
- token 每次注入新生成，只进内存，不落 metadata。

## 13. 断联重连与自定义协议拉起（已定稿）

### 场景约束（与 Codex++ 的本质差异）
- SevnX 是**常驻托盘独立进程**：SevnX 退出**不**杀 Codex；SevnX 启动**不**假设 Codex 在跑。
- Codex++ 是 launcher 进程，helper 与 launcher 同生共死——它靠"进程同生共死"绕开了"宿主重启但 Codex 还在跑"的状态。SevnX 必须**额外实现"重启后重连"**。

### 断联判定
- SevnX 退出 → overlay server 停 → 横条 `fetch` 失败（ECONNREFUSED）。
- 用"连续 N 次失败 + 超时"才置「已断联」，避免瞬时抖动误报。

### 断联 → 重连闭环
```
SevnX 退出 → overlay server 停 → 横条 fetch 失败 → 状态=已断联
     ↓ 用户点击横条「重新连接 SevnX」
     ↓ 触发 sevnx://relaunch?dbg=9229 自定义协议 → OS 拉起 sevnx.exe
     ↓ SevnX 启动 → 恢复 session → 读 metadata.debugPort
     ↓ list_targets 探测 Codex 仍在 → 注入新脚本(新overlayPort+新token) → 覆盖旧横条
     ↓ 横条恢复为实时数据
```

### 重连触发策略（统一为一条启动逻辑）
- SevnX **每次启动都尝试探测 debugPort 并重注入**（不带协议参数）。既覆盖"横条点击拉起"，也覆盖"用户手动打开 SevnX 而 Codex 已在跑"，最省心。

### 自定义协议分工（纠正"读注册表"误区）
- 浏览器 sandbox 限制：注入 JS **读不了注册表、执行不了 exe**。故"读注册表拿安装路径再 exec"在 JS 侧不可行。
- 横条 JS 唯一能拉起外部进程的通道 = OS 级系统触发（自定义 URI 协议）。
- 注册表只承担"协议→exe"映射（`HKCU\Software\Classes\sevnx\shell\open\command = "…\sevnx.exe" "%1"`），路径映射由 OS 完成，横条 JS 全程接触不到路径。
- SevnX 自装路径用 `std::env::current_exe()` 即可，Rust 侧无需读注册表。

### 实测方式（分两步）
1. **Rust/系统侧**：`reg add` 注册协议 → `start sevnx://relaunch?dbg=9229` 拉起 exe 并收到参数。
2. **Codex 侧**：用 CDP 触发横条 JS 动作，观察三件事：
   - 是否弹「打开此应用」确认框（Electron 默认会弹）；
   - 是否导致当前 Codex 页面被导航/报错——**用 `window.open` 或隐藏 `<a>`+click，勿用 `location.href = …`**；
   - 是否成功拉起 sevnx.exe。

### 确认框处理（Electron 同类）
- `ExternalProtocolDialogShowAlwaysOpenCheckbox`：给确认框加"始终允许"勾选。
- `AutoOpenAllowedForURLs` / `AutoOpenProtocolsFromOrigins`：对 `app://-` 来源直接放行，不弹框。
- 通过已有的 codex 启动参数通道注入（复用 `--remote-debugging-port` 之外的命令行参数）。

### 备选方案对比
| 方案 | 可行性 | 备注 |
| --- | --- | --- |
| 自定义协议 `sevnx://relaunch` | ✅ 推荐 | 标准可靠；注册表映射由 OS 处理；带 `?dbg=` 直接传端口 |
| 读注册表拿路径再 exec | ❌ JS 侧不可行 | sandbox 限制；Rust 侧自装用 `current_exe()` 即可 |
| 文件关联（`.sevnxlink`） | ⚠️ 冗余 | 需维护关联，`file://` 在 Electron 可能被拦 |
| Electron `shell.openExternal` | ⚠️ 不建议 | 需侵入 Codex 的 ipcRenderer，脆弱 |

### 结论
横条侧只保留自定义协议一条主通道：`window.open` / 隐藏 `<a>` 触发、带 `?dbg=` 传端口，配合 `AutoOpenProtocolsFromOrigins` 策略免确认框。

### 实测结论（已完成验证）
- 协议注册结构（HKCU 用户级，无需管理员）：
  - `HKCU\Software\Classes\sevnx` 默认值 `URL:sevnx Protocol`
  - `HKCU\Software\Classes\sevnx` 下 `URL Protocol`（REG_SZ 空值）——**缺它才报"需要新的应用"**
  - `HKCU\Software\Classes\sevnx\shell\open\command` 默认值 = `"<exe路径>" "%1"`
- **exe 文件名是连字符 `sevnx-monitor.exe`**（release 目录），dll 才是下划线 `sevnx_monitor.dll`，勿混用。
- 触发用 cmd 的 `start "" "sevnx://relaunch?dbg=9229"`（ShellExecute）；PowerShell 的 `Start-Process`/`.NET Process.Start` 对协议 URL 不可靠。
- 路径无空格时可省略 exe 的内嵌引号；cmd 里 `^"` 转义引号不可靠，路径有空格时应改用安装器变量或 `winreg` 自写。

### SevnX 侧 argv 监听设计（协议传来的数据在 SevnX 这边解析）
Windows 通过协议拉起 exe 时，把**完整 URL 作为命令行参数**传给 `sevnx-monitor.exe`（`sevnx://relaunch?dbg=9229` 整串进 argv）。SevnX 覆盖两种场景：
| 场景 | 处理位置 | 动作 |
| --- | --- | --- |
| SevnX **未运行**，协议首次拉起 | 启动时读 `std::env::args()` | 识别 `sevnx://relaunch?dbg=PORT` → 提取 PORT → 走"启动后探测该端口并重注入" |
| SevnX **已在运行**（托盘常驻），再次触发协议 | `single-instance` 插件 `on_new_instance` 回调（`src-tauri/src/lib.rs` 已有，L25-27） | 同样解析 argv → 触发重连注入 |

### 安装器注册协议（正式分发，替代命令行）
- **推荐**：安装器写 + 卸载器删。
  - NSIS：`WriteRegStr` 写入 `HKCR\...\sevnx` 三处 / 卸载 `DeleteRegKey "HKCU\Software\Classes\sevnx"`。
  - WiX：`<RegistryValue>` 元素 / 卸载自动清理。
  - 路径用安装目录变量（如 Tauri 的 `$INSTDIR\sevnx-monitor.exe`），兼容安装路径含空格。
- **次选**：Rust 首次启动用 `winreg` crate 自注册（检测未注册则写）；卸载时安装器需额外清理，否则可能残留。

## 14. 补充未讨论点（健壮性 / 安全 / 体验 / 生命周期）

### 架构与健壮性
1. **多窗口/target**：Codex 可能有主窗口 + quick-chat / avatar-overlay 页面（参考 Codex++ `cdp.rs`），只注入主界面，防重复注入。
2. **重注入 watchdog**：Codex 刷新/路由切换/重启后注入丢失，需周期检查 bridge 健康并重注入（复用 Codex++ `bridge_health_ok` + `start_bridge_watchdog` 骨架）。
3. **注入幂等去重**：类似 Codex++ `removeDuplicateCodexPlusMenus`，防按钮重复注入。
4. **选择器脆弱性**：Codex 升级可能改 DOM，`findNativeMenuInsertionPoint` 选择器会失效，需 floating 兜底 + 预留更新点。
5. **端口冲突 / 附加已运行实例**：调试端口被占用时的回退；Codex 已运行但未带调试端口时的处理。

### 安全与合规
6. **CORS/CSP**（已在 §12 定稿）：token 在 header，`Access-Control-Allow-Origin: *` 才安全；若改 Origin 白名单需同步收紧。
7. **token 管理**（已在 §12 定稿）：随机 token 安全生成、绑定进程生命周期轮换、只 bind loopback、只进内存不落盘、绝不写日志。
8. **白名单 payload**：overlay 只出三个量 + auth + 时间戳，不泄露 `api_keys`、user ID 等。

### 体验与降级
9. **刷新频率**（已定稿，见 §5「数据推送通道」）：long-poll 推送为主，轮询为降级兜底。
10. **错误态展示**：auth 失效 / 网络失败 / 数据为 null 时分别显示（`--`、stale 时间戳、重试）；**SevnX 退出时横条置「已断联」并可点击重拉**（见 §13）。
11. **未安装 Codex / 找不到 app_dir**：快捷方式点击后的引导提示。

### 生命周期与分发
12. **卸载/清理**：移除快捷方式、停止注入、清理 metadata 文件、**删除 `sevnx://` 协议注册**。
13. **打包安装**：参考 Codex++ `CodexPlusPlus.nsi` 集成快捷方式、启动项、**`sevnx://` 协议注册**。

### 测试
14. **单元测试**：cdp target 解析、cookie 续期逻辑、overlay payload 白名单（断言不含敏感字段）。
15. **手动验证矩阵**：深浅色切换、多窗口、Codex 刷新后重注入、登录/登出状态切换、**SevnX 退出→横条断联→点击协议重拉→重注入恢复**。

> 其中第 1、6、7 条（多 target、CORS、token 安全）是最易踩坑、建议优先设计；第 6、7 条已定稿，见 §12。

---

## 15. 落地顺序（分两步，各自可独立验证）

1. **第一步：本地 HTTP 暴露层**（不动注入）——新增 server + 端点 + 安全，
   用 `curl http://127.0.0.1:<port>/sevnx/overlay` 验证数据正确且不泄露敏感字段。
2. **第二步：注入脚本 + 数据联通 + 界面**——CDP 注入、按钮/横条、轮询渲染、登录动作、重注入 watchdog。
3. **第三步（建议穿插）：假 Codex 联调**——先用裸 Chromium/Electron 带 `--remote-debugging-port` 验证
   CDP 注入 + fetch 本地 HTTP + 断联重拉协议，再动真 Codex，降低迭代成本。
4. **第四步：断联重拉验证**——注册 `sevnx://relaunch` 协议 → 退出 SevnX → 横条置「已断联」→
   点击触发协议 → 确认拉起 exe 并拿到 `?dbg=` 参数 → SevnX 启动后探测重注入恢复。

## 16. 风险与边界

- **未带调试端口的 Codex 无法注入**：需由 SevnX 负责启动 Codex（带 `--remote-debugging-port`）或附加已有实例。
- **CSP/网络**：Codex 渲染进程对 `127.0.0.1` 的 fetch 可行（Codex++ helper 模式已验证）。
- **数据语义**：注入的是「余额/消费/用量」，不是"配额余量"；SevnX 后端当前无配额字段，日后可扩展端点。