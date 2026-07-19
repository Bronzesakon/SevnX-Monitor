# SevnX Monitor

SevnX Monitor 是一个独立的 Windows 10/11 桌面应用，用于查看 SevnX 账户余额、Dashboard 指标和聚合使用情况。应用使用 Tauri 2、Rust、Vue 3、Pinia、ECharts 和应用内 WebView2 登录。

## 功能概览

### 账户总览

- Dashboard 将余额与今日消费、今日与累计 Token 作为重点信息展示，同时汇总请求量、API 密钥、RPM/TPM 和平均响应时间。
- 数值采用统一的金额、Token 和耗时格式，长数值保留完整提示，便于快速识别账户状态与使用规模。
- 启动时自动恢复已保存会话，并在后台统一更新 Dashboard 与使用记录；刷新期间保留完整界面和状态提示，网络波动时保留会话和最后一次有效数据。

### 使用分析

- 使用记录支持多个时间范围，可从 Token、消费、模型和分组维度查看聚合结果。
- 趋势图展示时间序列变化，分布图和数据表帮助定位主要模型与分组的资源消耗。
- 使用记录界面与图表资源会在应用启动后预先准备，首次切换时保持完整布局；手动刷新与后台自动刷新协同工作，界面始终标明最近更新时间和刷新状态。

### 桌面体验

- 支持浅色、深色与跟随系统主题，以及开机启动、刷新频率和低余额提醒等偏好设置。
- 关闭主窗口后应用继续在系统托盘运行；左键托盘图标或菜单均可快速恢复窗口、刷新数据和打开设置。
- 吸附横条以置顶紧凑视图展示余额、今日消费和今日 Token；可从任意内容区域拖动并自动吸附屏幕边缘，在 DPI、分辨率或显示器布局变化后也会保持在可用工作区内。
- Windows 安装包提供标准安装与卸载体验，应用图标适配任务栏、系统托盘和高分辨率显示环境。

## 正式目录

- 仓库仅包含构建和运行所需的完整源代码、图标资源与构建脚本。
- `src-tauri/`：Rust 后端、Tauri 窗口、WebView2 登录、DPAPI 凭证库、托盘、通知和 NSIS 配置。
- `frontend/`：Vue 3 + TypeScript 用户界面；生产构建产物是 `frontend/dist/`。
- `dev-build.bat`：根目录本地构建入口，构建前后端产物后直接启动 debug EXE。
- `release-build.bat`：根目录发布构建入口，生成 release EXE 和 NSIS 安装包。

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

## 本地构建

在项目根目录执行：

```powershell
Set-Location E:\sevnX
.\dev-build.bat
```

该脚本依次执行 TypeScript 检查、生产前端构建与 Rust debug 构建，然后启动 `src-tauri/target/debug/sevnx-monitor.exe`。

## 发布构建

在项目根目录执行：

```powershell
Set-Location E:\sevnX
.\release-build.bat
```

该脚本执行 TypeScript 检查，再运行 `cargo tauri build --bundles nsis`。发布二进制位于 `src-tauri/target/release/sevnx-monitor.exe`，NSIS 安装包位于 `src-tauri/target/release/bundle/nsis/`。安装包使用应用标识 `com.sevnx.monitor`。

## 安全与数据边界

- 登录在应用创建的独立 WebView2 窗口内完成；打开官方 Dashboard 才交给系统默认浏览器。
- Cookie/Token 仅保留在 Rust 内存和 `%LOCALAPPDATA%\SevnX Monitor\auth\session.bin` 的 DPAPI CurrentUser 密文中，绝不通过 IPC、前端、日志或诊断信息输出。
- Dashboard 和 Usage 聚合快照仅存在于当前进程；不请求逐条 `/api/v1/usage` 接口。
- 普通网络错误保留最后一次成功快照和登录状态；只有 401、403 或明确的未登录业务码会清除会话。
- 日志和诊断信息仅保留脱敏后的公开元数据。
