# SevnX Monitor

**版本：1.0.0**

SevnX Monitor 是一个独立的 Windows 10/11 桌面应用，用于查看 SevnX 账户余额、Dashboard 指标和聚合使用情况。应用使用 Tauri 2、Rust、Vue 3、Pinia、ECharts 和应用内 WebView2 登录。

## 功能概览

### 账户总览

- Dashboard 将余额与今日消费、今日与累计 Token 作为重点信息展示，同时汇总请求量、API 密钥、RPM/TPM 和平均响应时间。
- 数值采用统一的金额、Token 和耗时格式，长数值保留完整提示，便于快速识别账户状态与使用规模。
- 启动时自动恢复已保存会话，并在后台统一更新 Dashboard 与使用记录；刷新期间保留完整界面和状态提示，网络波动时保留会话和最后一次有效数据。

### 登录与会话续期

- 登录在应用创建的独立 WebView2 窗口中完成，凭据由 Rust 后端捕获并验证，前端只接收脱敏后的状态和业务数据。
- 会话文件使用 Windows DPAPI CurrentUser 加密保存。恢复会话后每次启动都会尝试一次 `refresh_token` 续期，即使旧 `access_token` 已经过期也会直接使用独立保存的 refresh token。
- 后续按当前最新过期时间提前 30 分钟续期；每次服务端返回新的过期时间或轮换后的 refresh token，下一轮计时都会以新值重新计算。
- 续期失败会记录脱敏原因。凭据缺失或被服务端拒绝时，状态会同步为“登录失效”，横条和展开详情立即更新，不继续显示“已登录”。设置页提供手动“测试令牌续期”和重新登录入口。

### 使用分析

- 使用记录支持多个时间范围，可从 Token、消费、模型和分组维度查看聚合结果。
- 趋势图展示时间序列变化，分布图和数据表帮助定位主要模型与分组的资源消耗。
- 使用记录界面与图表资源会在应用启动后预先准备，首次切换时保持完整布局；手动刷新与后台自动刷新协同工作，界面始终标明最近更新时间和刷新状态。

### 桌面体验

- 支持访问网址、浅色/深色/跟随系统主题、开机启动、独立吸附横条和低余额提醒等偏好设置。
- 关闭主窗口后应用继续在系统托盘运行；左键托盘图标或菜单均可快速恢复窗口、刷新数据和打开设置。
- 吸附横条以置顶紧凑视图展示余额、今日消费和今日 Token；可从任意内容区域拖动并自动吸附屏幕边缘，在 DPI、分辨率或显示器布局变化后也会保持在可用工作区内。
- Windows 安装包提供标准安装与卸载体验，应用图标适配任务栏、系统托盘和高分辨率显示环境。

### 诊断与可维护性

- 关键生命周期写入结构化日志，包括协议注册、服务初始化、会话恢复/续期、Codex 激活、CDP 注入、渲染器重载、watchdog 重试和快捷方式创建。
- 日志只保留事件名、状态、端口和错误类别等公开元数据；Cookie、access token、refresh token、Authorization header 和路径中的敏感参数都会脱敏。
- overlay 服务仅绑定 `127.0.0.1` 随机端口，并以修订号推送最新快照，不开放局域网访问。

### Codex 集成（注入余额横条）

- 向 OpenAI Codex 桌面应用注入一个顶部横条，实时显示 **余额 / 今日消费 / 今日 Token**，点击可展开详情状态窗。
- 通过 CDP（`--remote-debugging-port=9229`）注入 UI 脚本，数据由 Rust 侧**主动推送**进页面，绕开 Codex 页面的 CSP 限制。
- 支持两种启动形态：Microsoft Store / MSIX 包用 COM 激活，独立安装版直接启动 exe；启动参数自动带上 `--remote-allow-origins`。
- Store 版的 AUMID **从包注册信息动态推导**，不写死：包名兼容新版 MSIX 的 `~` resource id 形式，并在同机并存时优先当前宿主 `OpenAI.ChatGPT-Desktop`，其次 `OpenAI.Codex` / `OpenAI.CodexBeta`；每次启动重新查询，Store 更新后不会继续命中旧版本目录。
- 横条优先直接插入 Codex header 的既有工具栏按钮组、排在第一个原生按钮之前；无项目页的按钮组缺失或裁切时改为右端锚定并向左展开，避免向右截断。真实 Codex 的窗口控制按钮位于原生标题栏（DOM 之外），故不覆盖三按钮。
- **反向拉起**：注册 `sevnx://relaunch` 自定义协议，并可在桌面 / 开始菜单创建快捷方式；点击后拉取 SevnX 再注入横条。
- 点击「打开带状态窗的 Codex」时检测运行状态：已带调试端口则只注入；宿主在运行但未开调试端口时先尝试激活，仍拿不到端口则重启宿主一次再拉起；未运行则启动并注入。保活 watchdog 每 5 秒检查并自动重注入。
- 重启宿主时只结束 Codex 自身进程（`Codex.exe` 按名字，`ChatGPT.exe` 仅在 Codex Store 包路径内），独立安装的普通 ChatGPT 客户端不受影响。

启动过渡阶段（Codex 空白页、渲染器重载或 header 尚未生成）不会显示右上角浮动横条；待稳定 header 或工具栏可用后才插入，避免遮挡和裁切。

## 正式目录

| 路径 | 作用 |
| --- | --- |
| `frontend/` | Vue 3 + TypeScript + Pinia 用户界面；包含仪表盘、使用记录、设置、登录引导、吸附横条和图表组件。 |
| `frontend/src/stores/app.ts` | 应用状态、刷新调度、登录、Codex 启动和快捷方式 IPC 调用。 |
| `src-tauri/src/app.rs` | 后端服务容器、会话恢复、启动/临期令牌续期、设置和状态快照。 |
| `src-tauri/src/api/` | SevnX API 客户端、响应解析和公开错误分类。 |
| `src-tauri/src/auth/` | WebView2 登录、请求观察、Cookie/Token 捕获、会话验证与 DPAPI 凭证存储。 |
| `src-tauri/src/model/` | Dashboard、Usage、设置和应用状态模型及格式化逻辑。 |
| `src-tauri/src/services/auto_refresh.rs` | 后台数据刷新循环。 |
| `src-tauri/src/services/refresh_scheduler.rs` | Dashboard 与 Usage 的并发刷新门控和结果合并。 |
| `src-tauri/src/services/codex_launcher.rs` | Store/独立版 Codex 探测、激活、启动参数和进程检测。 |
| `src-tauri/src/services/codex_inject.rs` | CDP 目标发现、脚本注入、异常诊断和 watchdog。 |
| `src-tauri/src/services/overlay_server.rs` | 本地回环数据推送、登录/重连动作和状态轮询接口。 |
| `src-tauri/src/services/protocol.rs` | `sevnx://relaunch` 协议注册、校验和参数解析。 |
| `src-tauri/src/services/shortcut.rs` | 桌面与开始菜单快捷方式创建。 |
| `src-tauri/resources/overlay.js` | Codex 页面内的横条、详情面板、原生样式提示和定位逻辑。 |
| `src-tauri/src/services/tray.rs` | 系统托盘图标和菜单。 |
| `src-tauri/src/services/logging.rs` | 脱敏结构化日志。 |
| `src-tauri/tauri.conf.json` | Tauri 窗口、权限、图标和 NSIS 打包配置。 |
| `.github/workflows/build.yml` | 正式编译入口：Windows CI 构建 NSIS 安装包并上传产物。 |
| `dev-build.bat` | 历史本地脚本；本地缓存已清理，仅作参考。 |
| `release-build.bat` | 历史本地脚本；正式发布改由 CI 执行。 |

## 开发环境

在 Windows 10/11 x64 上准备：

- Rust stable（MSVC 工具链）
- Node.js 20 或更新版本
- Microsoft Edge WebView2 Evergreen Runtime
- NSIS（仅在需要生成安装包时）

安装并构建前端：

```powershell
Set-Location E:\sevnX\frontend
npm ci
npm run build
```

验证 Rust 后端：

```powershell
Set-Location E:\sevnX\src-tauri
cargo fmt --all --check
cargo test
cargo check
```

`cargo tauri dev` 用于前端热更新和桌面应用开发。

```powershell
Set-Location E:\sevnX\src-tauri
cargo tauri dev
```

正式运行请使用发布二进制或 NSIS 安装包。

## CI 构建（正式编译入口）

编译统一由 `.github/workflows/build.yml` 在 GitHub Actions 的 `windows-latest` 上完成，本地不再作为构建环境。

触发方式：

| 触发 | 行为 |
| --- | --- |
| push 到 `main` | 构建并把安装包发布/更新到滚动 release `latest`（标记为预发布） |
| push `v*` 标签 | 构建并发布对应标签的正式 release |
| Pull Request 到 `main` | 仅构建验证，不发布 |
| 手动 `workflow_dispatch` | 按需构建；在 `main` 上会更新滚动 release |

产物：安装包以 **Release 附件**形式发布，下载下来就是 `.exe`（不再有 zip）。上传前会改名为无空格的文件名，因为 GitHub 会把附件名里的空格改写成点。

| 来源 | 下载地址 |
| --- | --- |
| 最新 main 构建（滚动 `latest` 预发布） | `releases/download/latest/sevnx-monitor-setup.exe` |
| `v*` 标签发布 | `releases/download/<标签>/sevnx-monitor-<标签>-setup.exe` |

> 不使用 workflow artifact：GitHub 的 artifact 一律以 zip 归档交付，无法直接下载裸 exe。

速度相关配置：

- `concurrency` 取消同一分支上被取代的旧构建，避免重复占用运行器。
- `actions/cache` 缓存 npm 包，`Swatinem/rust-cache` 缓存 `src-tauri/target` 与 cargo registry，未改动 `Cargo.lock` 时可跳过绝大部分依赖编译。
- `actions/cache` 复用 Tauri CLI 已下载的 NSIS 打包工具链。
- `CARGO_INCREMENTAL=0`，减小缓存体积并提升干净构建速度。
- 前端只构建一次：`tauri build` 的 `beforeBuildCommand` 已包含 `vue-tsc` 类型检查和 Vite 构建，工作流不再重复执行。

## 本地构建（已停用）

本地 `src-tauri/target`、`frontend/node_modules`、`frontend/dist` 及 npm/pnpm 缓存均已清理，目的是让 CI 成为唯一编译入口。`dev-build.bat` 与 `release-build.bat` 作为历史参考保留，执行前需要重新安装完整工具链并重建全部依赖：

```powershell
Set-Location E:\sevnX
.\release-build.bat
```

安装包使用应用标识 `com.sevnx.monitor`，产品版本为 `1.0.0`。

## 安全与数据边界

- 登录在应用创建的独立 WebView2 窗口内完成；打开官方 Dashboard 才交给系统默认浏览器。
- Cookie/Token 仅保留在 Rust 内存和 `%LOCALAPPDATA%\SevnX Monitor\auth\session.bin` 的 DPAPI CurrentUser 密文中，绝不通过 IPC、前端、日志或诊断信息输出。
- Dashboard 和 Usage 聚合快照仅存在于当前进程；不请求逐条 `/api/v1/usage` 接口。
- 普通网络错误保留最后一次成功快照和登录状态；只有 401、403 或明确的未登录业务码会清除会话并让前端与注入横条同步显示登录失效。
- 日志和诊断信息仅保留脱敏后的公开元数据。

## 版本

应用、Tauri bundle 和前端包统一使用 `1.0.0`。依赖包版本仍由各自的锁文件管理，不代表 SevnX Monitor 的产品版本。
