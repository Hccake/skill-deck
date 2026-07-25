---
status: accepted
---

# Window ACL 是 defense-in-depth，不是业务信任域

Main 与 Install Wizard 属于同一应用的 UI 分区，不建立两套独立的业务 command trust domain；二者共享应用级 command capability，仍由 Tauri default-deny、CSP、sanitizer 和实际使用的 plugin resource scope 提供外围约束。只有确实依赖窗口身份的 lifecycle/request command 保留 caller-window 校验，业务 authorization、typed identity、revision、路径和 ownership 始终由 Backend 负责。这样保留 ACL 对 WebView blast radius 的防御价值，同时避免维护容易漂移的窗口业务权限矩阵。
