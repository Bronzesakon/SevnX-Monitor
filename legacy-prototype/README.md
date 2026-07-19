# 早期研究原型（不参与正式构建）

本目录保留字段采集阶段的 Node、Vue、Playwright 和本地 HTTP 服务原型，
仅用于追溯已验证的接口字段与登录思路。

SevnX Monitor 的正式实现位于项目根目录的 `src-tauri/` 与 `frontend/`：

- 不得从本目录复用产品代码；
- 不得启动本目录中的 HTTP 服务、SSE 或浏览器调试逻辑；
- 不得将本目录纳入 Tauri 打包产物。

唯一的需求基线是项目根目录的 `PROJECT_SPEC.md`。
