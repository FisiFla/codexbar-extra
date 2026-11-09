# CodexBar for Linux — Rust rebuild plan (mac parity)

Status: approved direction (Rust from scratch, macOS app untouched).
Location: `Integrations/LinuxRust/` (Qt app in `Integrations/Linux/` stays until parity, then packaging swaps).

## Status (2026-11-09, first release `linux-v0.84.0`)

Shipped: tray (SNI meters/icon), Quick View popover (switcher, provider cards, pace, cost, DeepSeek clock),
Usage & Spend (totals, trend, heatmap, provider/model/ledger tabs, usage cards), Settings shell with search and
all panes, provider detail with enable/disable + API key/cookie via CLI (stdin), notifications, autostart,
Qt-compatible argv/IPC/`linux.json`/tarball, Omarchy adapter. Verified live against the CLI on COSMIC.

`linux-v0.84.1` adds: Agent Sessions window (`--sessions`, tray, popover; `sessions --json-v2`), Share Stats window
(1200×630 card, Save PNG to Pictures, Copy image via wl-copy/xclip), Menu Bar layout editor (tokens, conditionals, presets,
live preview; stored in `~/.config/codexbar/menubar-layout.json`, linux.json untouched), popover Cost history / Model
breakdown chart sections and hover tooltips on all bar charts, model ranking in Spend/Share from per-day breakdowns.

Open: OpenAI Live window (Admin API key), credits/utilization/burndown/storage/throughput charts (no Linux data source
in the CLI yet), local session/tool spend scanners, quota overlay/confetti, session window focus, per-pane visual polish.
Dev aid: `CODEXBAR_SCREENSHOT_DIR=<dir>` captures every window/pane/tab as PNG and quits.

## Why the Qt app feels so far behind

- Size: mac app+core ≈ 311,609 lines of Swift (527 app files); Linux Qt frontend ≈ 2,467 lines (QML+C+++JS).
- Age/design: Linux desktop first landed upstream 2026-09-12 as a deliberately thin shell: it makes exactly 3 CLI calls
  (`usage`, `cost`, `config providers`) and renders a compact projection (`Integrations/Linux/Shared/Usage.js`).
  Everything the mac app computes in `UsageStore`/Core (throughput sparkline, credits/breakdown history, plan-utilization/burndown,
  spend session/tool detail, menu-bar token layout engine, ~111 provider descriptors) has no CLI-JSON equivalent today.
- `Integrations/Linux/MAC_COMPARISON.md` already documents the reduced settings/tray/accounts surface.

## Goal

A native Rust Linux app that is a drop-in replacement for `codexbar-linux` and replicates every user-facing
macOS screen of this fork (CodexBar Extra, incl. DeepSeek price clock/analytics) as closely as the Linux platform allows.

## Success criteria

1. Same binary name/argv/IPC/files as Qt version (Omarchy Panel.qml works unchanged).
2. Every surface in the parity matrix below exists with same content order, labels, states (loading/stale/error/unknown≠0).
3. Live data matches `codexbar usage/cost/dashboard` JSON on this machine (fixture contract tests).
4. Settings persist compatibly: `~/.config/codexbar/linux.json` (UI prefs) + shared `config.json` only via CLI/flocked writes.
5. Installable tarball + GitHub release from `FisiFla/codexbar-extra` (x86_64 + aarch64), macOS release flow untouched.

## Parity matrix (from 6-agent audit, 2026-11-08)

Surfaces: 1 tray icon engine (MenuBarLayout V7 tokens, critters/bars/iconAndPercent) · 2 main popover (switcher,
Overview, provider cards 310pt, account switchers, footer) · 3 hover chart submenus (credits/breakdown/history/
utilization/burndown/storage) · 4 settings shell (880×620, searchable sidebar, provider list) · 5 General ·
6 ~~iCloud Sync~~ (drop: CloudKit, no Linux equivalent) · 7 Usage & Spend (periods, heatmap, trend, rankings,
ledger, token mix, hourly, session browser) · 8 Notifications (→ org.freedesktop.Notifications) · 9 Menu Bar
(layout editor + conditionals) · 10 Menu · 11 Advanced (drive `codexbar serve`) · 12 Hooks (CLI-backed, portable) ·
13 Plugins (Rust QuickJS host later; list/fetch via CLI first) · 14 About/Acknowledgements (updater: unavailable) ·
15 Debug (reduced; `diagnose` JSON) · 16 Provider detail ×~113 (data-driven from descriptors, token accounts,
Codex managed accounts, claude-swap, DeepSeek profile picker) · 17 Share Stats window (PNG render/copy/save) ·
18 OpenAI Live Usage window (Admin API client) · 19 Workspaces Inspector (keep env-gated) ·
20 Quota overlay (layer-shell, degradable) · 21 Confetti overlay (degradable) · 22 Agent Sessions (CLI sessions v2;
focus best-effort) · 23 ~~WidgetKit~~ (drop) · 24 Existing Linux contracts (below — must not break).

Data rule: build on CLI JSON + local files, never port UsageStore:
`codexbar usage --format json --json-only` (60 s, 8 MiB), `codexbar cost --provider both --format json --days N`,
`codexbar config providers --format json`, plus `dashboard`, `sessions --json-v2`, `hooks`, `plugins`, `diagnose`, `serve`.
Gaps needing new Rust backends: tokens/min throughput, credits history, utilization/burndown history, storage scan,
full spend session/tool scanners (Codex rollouts, Claude transcripts, OpenCode SQLite).
Hard cliff: all browser-cookie auto-imports are macOS-only upstream (CLI fails closed) — Linux keeps API-key/manual-cookie.

## Contracts to preserve byte-for-byte

- Binary `~/.local/bin/codexbar-linux`; app id `com.steipete.CodexBar` desktop/autostart files.
- Argv: `--snapshot[--with-spending] --refresh --settings --spending --usage --quick-view --quit --background --no-tray --autostart <a> --configure <json> --cli <path>`.
- IPC: `$XDG_RUNTIME_DIR/codexbar-linux/desktop.sock`, newline JSON, snapshot `schemaVersion:1` (no emails ever).
- `linux.json` 24 keys + validation (extend compatibly; Qt rejects unknown keys on save — version carefully).
- Tarball `CodexBarDesktop-v<ver>-linux-{x86_64|aarch64}.tar.gz` + `.sha256`; Omarchy dir `steipete.codexbar`.

## Stack (Rust)

- GUI: `iced` (wgpu, Wayland+X11, Canvas for meters/charts/heatmap/token tray) — single binary, no WebKit/Qt runtime.
- Tray: `ksni` (StatusNotifierItem D-Bus); icon rasterized with `tiny-skia` (critters + V7 tokens, light/dark).
- Async/IPC: `tokio` + Unix socket; notifications & secrets via `zbus` (org.freedesktop.Notifications / Secret Service—only if CLI interop kept).
- Data: `serde`/`serde_json` typed CLI payloads; subprocess runner with timeout/caps; `rusqlite` for OpenCode/session history.
- Assets: reuse 109 `ProviderIcon-*.svg` + descriptor hex table (generate Rust table from Swift descriptors + `config providers`).

## Work plan (subagent work units; lead agent reviews each)

1. **Foundation** — workspace, typed CLI client + fixtures, settings store, IPC server/client, single-instance, tray meters, notifications. Verify: `cargo test`, snapshot golden vs Qt, Omarchy panel poll.
2. **Popover** — Quick View parity: switcher, Overview+spend summary, full provider card sections, DeepSeek clock, states/shortcuts. Verify: screenshot vs mac order checklist; live Codex+Claude.
3. **Spend** — Usage & Spend from cost JSON, then Rust local scanners for session/tool detail. Verify: totals match CLI cost; unknown stays `—`.
4. **Settings** — shell+search; General/Menu/Notifications/Hooks/Advanced/About/Debug; MenuBar layout editor; data-driven Provider detail + token/Codex accounts. Verify: round-trip config via CLI `config dump`.
5. **Extras** — Share Stats PNG, OpenAI Live, Agent Sessions, quota overlay/confetti (degraded modes ok), stay-awake (`systemd-inhibit`). Verify per-surface manual checks on COSMIC.
6. **Ship** — installer reuse, packaging, CI matrix (ubuntu-24.04 + arm), README repoint upstream→fork, release. Verify: clean-VM/user install per README, checksums.

Non-goals: iCloud pane, Sparkle/Homebrew updater, WidgetKit, Keychain prompts, browser-cookie auto-import.
Risks: descriptor extraction (~113 providers), spend scanner size, Wayland limits (global hotkey/focus/overlays), secrets interop with Swift CLI.
Rollback: Qt app remains the packaged default until phase-6 gate; Rust ships as same-named binary only after parity review.
