(() => {
  // The renderer can be recreated without the webview being destroyed. Keep
  // one bar and one set of listeners across repeated CDP injections.
  window.__sevnxOverlayCleanup?.();
  // Remove every leftover node, not just the first with each id. If an older
  // copy is still installed when this script is re-injected, removing only the
  // first match would strand a duplicate bar — and CSS id selectors would then
  // style every duplicate, making the leak look intentional.
  for (const id of ["sevnx-overlay-bar", "sevnx-overlay-detail", "sevnx-overlay-tooltip", "sevnx-overlay-style"]) {
    document.querySelectorAll(`[id="${id}"]`).forEach((node) => node.remove());
  }
  window.__sevnxOverlayInstalled = true;

  const DEBUG_PORT = 9229;
  const HEADER_SELECTOR = ".app-header-tint, header";
  const RIGHT_ANCHORED_CLASS = "sevnx-overlay-right-anchored";
  const LOADING_HIDDEN_CLASS = "sevnx-overlay-loading-hidden";
  // Codex publishes its design tokens as `--color-*` custom properties on
  // :root, so styling the bar with them makes it follow the live theme —
  // including light/dark switches — instead of guessing a theme. The previous
  // `--token-*` names do not exist in Codex, so every declaration silently fell
  // back to its light literal and the bar rendered white on the dark UI.
  // (Verified against Codex 26.903: --color-token-main-surface-primary #181818,
  // --color-token-foreground #dfdfdf in dark mode.)
  const css = `
    #sevnx-overlay-bar {
      display: inline-flex; align-items: center; gap: 12px; height: 30px;
      flex: 0 0 auto; box-sizing: border-box; padding: 0 12px;
      color: var(--color-token-foreground, #dfdfdf);
      font: 12px/1 system-ui, -apple-system, "Segoe UI", sans-serif;
      white-space: nowrap;
      background: var(--color-token-main-surface-primary, var(--color-surface, #181818));
      border: 1px solid var(--color-token-border-default, rgba(255, 255, 255, .084));
      border-radius: 8px;
      box-shadow: none; cursor: pointer; user-select: none;
      pointer-events: auto; -webkit-app-region: no-drag;
    }
    #sevnx-overlay-bar.${RIGHT_ANCHORED_CLASS} {
      position: fixed; z-index: 2147483645;
      top: var(--sevnx-top, 8px); right: var(--sevnx-right, 8px);
      height: var(--sevnx-height, 30px);
    }
    #sevnx-overlay-bar.${LOADING_HIDDEN_CLASS} { display: none; }
    #sevnx-overlay-bar .sevnx-metric { display: inline-flex; align-items: baseline; gap: 4px; }
    #sevnx-overlay-bar .sevnx-metric b { font-weight: 600; }
    #sevnx-overlay-bar .sevnx-metric span { font-size: 10px; color: var(--color-token-description-foreground, rgba(255, 255, 255, .498)); }
    #sevnx-overlay-bar .sevnx-caret { color: var(--color-token-description-foreground, rgba(255, 255, 255, .498)); font-size: 11px; transition: transform .15s ease; }
    #sevnx-overlay-bar.sevnx-open .sevnx-caret { transform: rotate(180deg); }
    #sevnx-overlay-detail {
      position: fixed; z-index: 2147483646; width: 260px; box-sizing: border-box;
      background: var(--color-token-dropdown-background, var(--color-token-main-surface-primary, #2d2d2d));
      border: 1px solid var(--color-token-border-default, rgba(255, 255, 255, .084));
      border-radius: 10px; padding: 14px; box-shadow: none;
      color: var(--color-token-dropdown-foreground, var(--color-token-foreground, #dfdfdf));
      font: 13px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif;
      display: none; cursor: default; -webkit-app-region: no-drag;
    }
    #sevnx-overlay-detail .sevnx-row { display: flex; justify-content: space-between; gap: 12px; padding: 3px 0; }
    #sevnx-overlay-detail .sevnx-row span { color: var(--color-token-description-foreground, rgba(255, 255, 255, .498)); }
    #sevnx-overlay-detail .sevnx-sec { margin-top: 10px; padding-top: 8px; border-top: 1px solid var(--color-token-border-default, rgba(255, 255, 255, .084)); }
    #sevnx-overlay-detail #sevnx-relaunch {
      display: block; box-sizing: border-box; margin-top: 10px; width: 100%; padding: 6px;
      border: 1px solid var(--color-token-border-default, rgba(255, 255, 255, .084)); border-radius: 8px;
      background: transparent; color: inherit; cursor: pointer; text-align: center; text-decoration: none;
    }
    #sevnx-overlay-tooltip {
      position: fixed; z-index: 2147483647; display: none; box-sizing: border-box; width: fit-content;
      max-width: min(20rem, calc(100vw - 16px)); padding: 4px 8px;
      border: 1px solid var(--color-token-border-default, rgba(255, 255, 255, .084)); border-radius: 12.5px;
      background: var(--color-token-dropdown-background, var(--color-token-main-surface-primary, #2d2d2d));
      box-shadow: none;
      color: var(--color-token-foreground, #dfdfdf);
      font: 445 13px/18.5714px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      white-space: normal; overflow-wrap: break-word; user-select: none;
      pointer-events: none; -webkit-app-region: no-drag;
    }
  `;
  const style = document.createElement("style");
  style.id = "sevnx-overlay-style";
  style.textContent = css;
  document.head.appendChild(style);

  const bar = document.createElement("div");
  bar.id = "sevnx-overlay-bar";
  bar.setAttribute("role", "button");
  bar.setAttribute("aria-label", "显示摘要");
  bar.tabIndex = 0;
  bar.innerHTML = `
    <span class="sevnx-metric"><b id="sevnx-balance">--</b><span>余额</span></span>
    <span class="sevnx-metric"><b id="sevnx-spend">--</b><span>今日消费</span></span>
    <span class="sevnx-metric"><b id="sevnx-tokens">--</b><span>今日 Token</span></span>
    <span class="sevnx-caret" aria-hidden="true">⌄</span>
  `;
  const detail = document.createElement("div");
  detail.id = "sevnx-overlay-detail";
  detail.innerHTML = `
    <div class="sevnx-row"><span>状态</span><b id="sevnx-auth">--</b></div>
    <div class="sevnx-row"><span>余额</span><b id="d-balance">--</b></div>
    <div class="sevnx-row"><span>今日消费</span><b id="d-spend">--</b></div>
    <div class="sevnx-row"><span>今日 Token</span><b id="d-tokens">--</b></div>
    <div class="sevnx-sec">
      <div class="sevnx-row"><span>今日请求</span><b id="d-requests">--</b></div>
      <div class="sevnx-row"><span>累计 Token</span><b id="d-cumulative">--</b></div>
      <div class="sevnx-row"><span>输入 / 输出 Token</span><b id="d-io">--</b></div>
      <div class="sevnx-row"><span>RPM / TPM</span><b id="d-rpm">--</b></div>
      <div class="sevnx-row"><span>平均响应</span><b id="d-avg">--</b></div>
    </div>
    <div class="sevnx-row"><span>更新时间</span><b id="d-ts">--</b></div>
    <a id="sevnx-relaunch" href="sevnx://relaunch?dbg=${DEBUG_PORT}" role="button" style="display:none">重新连接 SevnX</a>
  `;
  const tooltip = document.createElement("div");
  tooltip.id = "sevnx-overlay-tooltip";
  tooltip.setAttribute("role", "tooltip");
  tooltip.textContent = "显示摘要";
  document.documentElement.append(detail, tooltip);

  const set = (id, value) => {
    const element = document.getElementById(id);
    if (element) element.textContent = value == null || value === "" ? "--" : String(value);
  };
  const visibleRect = (element) => {
    const rect = element?.getBoundingClientRect?.();
    return rect && rect.width > 0 && rect.height > 0 ? rect : null;
  };
  // Keep the native toolbar layout where it has room. Home/no-project views
  // can use a narrower or clipped group, so they fall back to a right-anchored
  // overlay that always grows leftward from the native controls.
  function findToolbarButtonGroup() {
    const header = document.querySelector(HEADER_SELECTOR);
    if (!header) return null;
    const groups = Array.from(header.querySelectorAll("[class*='ms-auto'][class*='flex'][class*='items-center']"));
    const group = groups.find((node) => visibleRect(node) && !node.closest(".invisible"));
    const groupButtons = Array.from(group?.querySelectorAll("button") || []).filter((button) => visibleRect(button));
    if (!group || !groupButtons.length) return null;
    let firstItem = groupButtons[0];
    while (firstItem.parentElement && firstItem.parentElement !== group) {
      firstItem = firstItem.parentElement;
    }
    return firstItem.parentElement === group ? { parent: group, firstItem } : null;
  }

  let fallbackTarget = undefined;
  let retryToolbarLayout = false;

  function sameFallbackTarget(target) {
    return fallbackTarget !== undefined
      && fallbackTarget.parent === (target?.parent || null)
      && fallbackTarget.firstItem === (target?.firstItem || null);
  }

  function visibleRightToolbarButtons() {
    const header = document.querySelector(HEADER_SELECTOR);
    if (!header) return [];
    return Array.from(header.querySelectorAll("button"))
      .map((button) => ({ button, rect: visibleRect(button) }))
      .filter(({ button, rect }) => rect
        && rect.left > window.innerWidth / 2
        && rect.right <= window.innerWidth + 1
        && !button.closest("#sevnx-overlay-bar"))
      .sort((left, right) => left.rect.left - right.rect.left);
  }

  function hideForLoadingPage() {
    fallbackTarget = undefined;
    bar.classList.remove(RIGHT_ANCHORED_CLASS, "sevnx-open");
    bar.classList.add(LOADING_HIDDEN_CLASS);
    detail.style.display = "none";
    tooltip.style.display = "none";
  }

  function placeRightAnchored() {
    const headerRect = visibleRect(document.querySelector(HEADER_SELECTOR));
    const buttons = visibleRightToolbarButtons();
    const anchor = buttons[0];
    let top = headerRect
      ? headerRect.top + Math.max(0, (headerRect.height - 30) / 2)
      : 8;
    let height = 30;
    let right = 8;
    if (anchor) {
      top = anchor.rect.top;
      height = anchor.rect.height;
      const measuredGap = buttons[1] ? buttons[1].rect.left - anchor.rect.right : 0;
      const style = anchor.button.parentElement ? getComputedStyle(anchor.button.parentElement) : null;
      const gap = Math.max(Number.parseFloat(style?.columnGap || style?.gap || "0") || 0, measuredGap, 0);
      right = Math.max(8, Math.round(window.innerWidth - anchor.rect.left + gap));
    }
    bar.style.setProperty("--sevnx-top", `${Math.round(top)}px`);
    bar.style.setProperty("--sevnx-height", `${Math.round(height)}px`);
    bar.style.setProperty("--sevnx-right", `${right}px`);
  }

  function activateRightAnchored(target) {
    fallbackTarget = {
      parent: target?.parent || null,
      firstItem: target?.firstItem || null,
    };
    bar.classList.add(RIGHT_ANCHORED_CLASS);
    if (bar.parentElement !== document.documentElement) document.documentElement.appendChild(bar);
    placeRightAnchored();
  }

  function toolbarBarIsClipped() {
    const rect = visibleRect(bar);
    return !rect
      || rect.left < 0
      || rect.right > window.innerWidth
      || bar.scrollWidth > bar.clientWidth + 1;
  }

  function installLayout() {
    const header = document.querySelector(HEADER_SELECTOR);
    const target = findToolbarButtonGroup();
    // The Codex splash renderer has neither a usable header nor native toolbar
    // buttons. Do not use the fixed fallback there: it would overlap the
    // window controls before the application page has loaded.
    if (!visibleRect(header) || (!target && !visibleRightToolbarButtons().length)) {
      hideForLoadingPage();
      return;
    }
    bar.classList.remove(LOADING_HIDDEN_CLASS);
    if (retryToolbarLayout) {
      fallbackTarget = undefined;
      retryToolbarLayout = false;
    }
    if (sameFallbackTarget(target)) {
      placeRightAnchored();
      if (detail.style.display !== "none") placeDetail();
      if (tooltip.style.display === "block") placeTooltip();
      return;
    }
    if (!target) {
      activateRightAnchored(null);
      return;
    }
    fallbackTarget = undefined;
    bar.classList.remove(RIGHT_ANCHORED_CLASS);
    if (bar.parentElement !== target.parent || bar.nextSibling !== target.firstItem) {
      target.parent.insertBefore(bar, target.firstItem);
    }
    requestAnimationFrame(() => {
      if (bar.parentElement === target.parent && toolbarBarIsClipped()) {
        activateRightAnchored(target);
      }
    });
    if (detail.style.display !== "none") placeDetail();
    if (tooltip.style.display === "block") placeTooltip();
  }

  function placeDetail() {
    const rect = visibleRect(bar);
    if (!rect) return;
    detail.style.top = `${Math.round(rect.bottom + 8)}px`;
    detail.style.right = `${Math.max(8, Math.round(window.innerWidth - rect.right))}px`;
  }
  function placeTooltip() {
    const rect = visibleRect(bar);
    if (!rect) return;
    const width = tooltip.offsetWidth;
    const height = tooltip.offsetHeight;
    const left = Math.min(
      Math.max(8, Math.round(rect.left + rect.width / 2 - width / 2)),
      Math.max(8, window.innerWidth - width - 8),
    );
    tooltip.style.left = `${left}px`;
    tooltip.style.top = `${Math.max(8, Math.round(rect.top - height - 2))}px`;
  }
  let layoutQueued = false;
  function scheduleLayout() {
    if (layoutQueued) return;
    layoutQueued = true;
    requestAnimationFrame(() => { layoutQueued = false; installLayout(); });
  }
  function isOverlayOnlyMutation(records) {
    const isOverlayTarget = (node) => node === bar
      || node === detail
      || node === tooltip
      || bar.contains(node)
      || detail.contains(node)
      || tooltip.contains(node);
    return records.every((record) => isOverlayTarget(record.target)
      || Array.from(record.addedNodes)
        .concat(Array.from(record.removedNodes))
        .every((node) => node === bar || node === detail || node === tooltip));
  }
  const observer = new MutationObserver((records) => {
    if (!isOverlayOnlyMutation(records)) retryToolbarLayout = true;
    scheduleLayout();
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
  const onResize = () => { retryToolbarLayout = true; scheduleLayout(); };
  window.addEventListener("resize", onResize);
  installLayout();
  for (let i = 1; i <= 10; i++) setTimeout(() => { retryToolbarLayout = true; installLayout(); }, i * 300);

  let lastPush = Date.now();
  let disconnected = false;
  function money(value) {
    const text = String(value ?? "").replaceAll(",", "").replace(/[¥￥$]/g, "").trim();
    const number = Number(text);
    return Number.isFinite(number) ? `¥${number.toFixed(2)}` : "--";
  }
  function compact(value) {
    const number = Number(value);
    return Number.isFinite(number) && number >= 1000000 ? `${(number / 1000000).toFixed(2)}M` : (value == null || value === "" ? "--" : String(value));
  }
  function integer(value) {
    const text = String(value ?? "").replaceAll(",", "");
    return /^\d+$/.test(text) ? Number(text).toLocaleString("en-US") : (value == null || value === "" ? "--" : String(value));
  }
  function authLabel(value) {
    const labels = {
      authenticated: "已登录",
      logged_out: "未登录",
      logging_in: "登录中",
      validating: "验证中",
      expired: "登录失效",
      disconnected: "已断联",
    };
    return labels[value] || (value == null || value === "" ? "--" : "状态未知");
  }
  function render(data = {}) {
    lastPush = Date.now();
    disconnected = false;
    set("sevnx-balance", money(data.balance)); set("sevnx-spend", money(data.todaySpend)); set("sevnx-tokens", compact(data.todayTokens));
    set("sevnx-auth", authLabel(data.auth)); set("d-balance", money(data.balance)); set("d-spend", money(data.todaySpend)); set("d-tokens", compact(data.todayTokens));
    set("d-requests", integer(data.todayRequests)); set("d-cumulative", compact(data.cumulativeTokens));
    set("d-io", `${compact(data.inputTokens)} / ${compact(data.outputTokens)}`); set("d-rpm", `${integer(data.rpm)} / ${integer(data.tpm)}`); set("d-avg", data.averageResponse);
    set("d-ts", data.fetchedAt ? new Date(data.fetchedAt).toLocaleTimeString() : "--");
    const relaunch = document.getElementById("sevnx-relaunch"); if (relaunch) relaunch.style.display = "none";
  }
  function markDisconnected() {
    if (disconnected) return;
    disconnected = true; set("sevnx-balance", "断开"); set("sevnx-spend", "--"); set("sevnx-tokens", "--"); set("sevnx-auth", authLabel("disconnected"));
    const relaunch = document.getElementById("sevnx-relaunch"); if (relaunch) relaunch.style.display = "block";
  }
  window.__sevnxOverlayPush = render;
  const disconnectTimer = setInterval(() => { if (Date.now() - lastPush > 15000) markDisconnected(); }, 3000);

  const isOpen = () => detail.style.display !== "none";
  const hideDetail = () => { detail.style.display = "none"; bar.classList.remove("sevnx-open"); };
  const showDetail = () => { placeDetail(); detail.style.display = "block"; bar.classList.add("sevnx-open"); };
  const showTooltip = () => { tooltip.style.display = "block"; placeTooltip(); };
  const hideTooltip = () => { tooltip.style.display = "none"; };
  bar.addEventListener("click", () => (isOpen() ? hideDetail() : showDetail()));
  bar.addEventListener("mouseenter", showTooltip);
  bar.addEventListener("mouseleave", hideTooltip);
  bar.addEventListener("focus", showTooltip);
  bar.addEventListener("blur", hideTooltip);
  bar.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      isOpen() ? hideDetail() : showDetail();
    }
  });
  const onDocumentClick = (event) => { if (isOpen() && !(event.target instanceof Element && event.target.closest("#sevnx-overlay-bar, #sevnx-overlay-detail"))) hideDetail(); };
  const onKeyDown = (event) => { if (event.key === "Escape") hideDetail(); };
  document.addEventListener("click", onDocumentClick);
  document.addEventListener("keydown", onKeyDown);
  window.__sevnxOverlayCleanup = () => {
    clearInterval(disconnectTimer); observer.disconnect(); window.removeEventListener("resize", onResize);
    document.removeEventListener("click", onDocumentClick); document.removeEventListener("keydown", onKeyDown);
    tooltip.remove();
    delete window.__sevnxOverlayPush; window.__sevnxOverlayInstalled = false;
  };
})();
