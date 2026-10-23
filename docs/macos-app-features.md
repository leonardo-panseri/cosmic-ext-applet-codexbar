# CodexBar macOS app — functional feature inventory

Source: [steipete/CodexBar](https://github.com/steipete/CodexBar) (explored 2026-07-25). This document catalogs
**what the macOS menu bar app does**, purely at the functional/UX layer — no architecture, no Swift/AppKit
implementation notes. Use it as the checklist when scoping which views/features the COSMIC applet should offer,
given it will sit on top of the same **Linux CLI backend** CodexBar already ships (`codexbar` — Homebrew/AUR/tarball,
reads `~/.config/codexbar/config.json`, works headless on Linux today).

Note: several unofficial Linux integrations already build on that same CLI (Waybar, GNOME Shell extension,
Cinnamon applet, KDE Plasma widgets, Noctalia/Quickshell) — evidence the CLI's `usage`/`cost`/`serve`/`cards` JSON
surface is a sufficient backend contract for a panel applet.

---

## 1. Core concept

A menu-bar-only app (no Dock icon) that shows **AI coding-tool usage/quota status** for many providers (63
registered: Codex/OpenAI, Claude, Cursor, Gemini, Copilot, Grok, GroqCloud, ElevenLabs, Deepgram, z.ai, MiniMax,
Kiro, Zed, Vertex AI, Augment, OpenRouter, LiteLLM, AWS Bedrock, and more — full list in `docs/providers.md`
upstream). Core value props:

- **Reset countdowns** — session/weekly/monthly usage windows with time-to-reset.
- **Credits & spend** — credit balances, admin-API spend dashboards, local cost scans.
- **Live status** — provider incident polling with a badge/overlay.
- **Privacy-first** — reuses existing sessions (OAuth, CLI creds, API keys, cookies); no passwords stored.

---

## 2. Menu bar presence (panel icon)

- One status item (tray/panel icon) **per enabled provider**, OR a single combined icon in **Merge Icons** mode.
- Icon is a small usage meter: fill represents % remaining (or % used, if flipped in settings).
- Icon dims / shows an incident glyph when the last refresh failed or a provider has an outage.
- Icon rendering styles: plain meter icon, "icon + percent" text, brand icon + title text ("token" style).
- A configurable **menu bar layout**: a token-based text composer (provider name, account, session %, weekly %,
  auto %, usage bar glyph, resets-in / reset-at / runs-out, cost-today / cost-30d, separators/line-breaks). Presets
  plus free-form drag-and-drop token editor. Can be set globally or per-provider. Small/Regular font scale,
  Tight/Regular padding, one- or two-line ("compact stacked") layout.
- Loading animation while refreshing, rate/duration-capped so a hung provider can't spin the icon forever.
- "Surprise me" — optional random icon blink/animation easter egg.
- High-contrast mode toggle for inactive/external displays.

## 3. Menu popover (click target) — per-provider card

Clicking a status item opens a popover/menu with:

- **Header**: provider name/brand icon, account identity (email/org), plan badge (e.g. "Pro").
- **Usage rows** for each window the provider exposes: primary (session), secondary (weekly), tertiary, "extra"
  (e.g. per-model sub-limits like Sonnet-specific quota). Each row: progress bar, % left, reset countdown
  (or absolute clock time, per setting).
- **Pace tracking** per window: compares actual vs. expected even-consumption rate.
  - States: "On pace", "X% in deficit" (running hot — shows an ETA "Runs out in …"), "X% in reserve" (headroom —
    shows "Lasts until reset").
  - A "Work days" setting (Automatic / 4 / 5 / 7) controls the weekly pace model; Automatic uses historical pace
    data (Codex) when enough history exists.
  - Hidden until ≥3% of the window has elapsed (not enough signal yet).
- **Credits row** (where applicable) — numeric credits remaining, optional "Buy Credits…" action.
- **Cost row(s)** — "Cost today" / "Cost 30d" local estimate, when the provider supports cost-history.
- **Manual refresh** — spinner in place, in-flight requests are shared/deduplicated across repeated clicks.
- **Provider-specific extras**: e.g. Codex/OpenAI web dashboard enrichments (code review quota remaining, daily
  usage breakdown chart, credit-purchase history) when that optional data source is attached.
- **Multi-account support**: an account switcher bar, or stacked account cards (up to 6), when multiple token
  accounts are configured for a provider.
- **Storage usage** (opt-in): local disk footprint of that provider's own config/cache files, with a submenu
  breakdown by path (copyable paths). Read-only — never deletes anything.
- **Status/incident line**: indicator + short description when status polling is enabled; click opens the
  provider's status page.
- **Provider changelog link** (optional toggle).
- Row for opening the provider's web dashboard / account page.
- Error state: collapsed error banner, expandable for full text, with a copy-to-clipboard action.

## 4. Merge Icons mode (combined provider switcher)

- Toggle that collapses all enabled providers into **one** status item.
- That single icon shows a **switcher**: buttons/rows to jump between providers, with an optional **Overview tab**
  (up to 6 chosen providers rendered as compact rows in one glance, ordered per provider order).
  - If no providers are chosen for Overview, the tab is hidden.
- "Show most-used provider" — auto-selects the highest-usage provider as the default icon/view.
- Switcher-row count setting (how many provider rows show at once).

## 5. Provider management (Settings → Providers)

- One settings row per registered provider (63), each independently toggleable, reorderable (drag, or "sort
  alphabetically" toggle), and searchable/filterable by name.
- Sidebar shows: provider brand icon, enabled/disabled dimming, in-flight refresh spinner, and a live status dot
  (colored by incident severity) when status checks are on.
- Per-provider **detail/settings pane**:
  - Enable/disable toggle.
  - **Source picker**: Automatic / Web / CLI / OAuth / API — which auth strategy to prefer.
  - **Cookie source picker** (cookie-based providers): Automatic (browser import) / Manual (paste `Cookie:` header)
    / Off.
  - **API key field** (API-based providers), stored in the local config file, not the OS keychain (keychain is
    reserved for browser-cookie decryption keys and OAuth/device-flow tokens).
  - Provider-specific extra fields: region/host selector (e.g. z.ai, MiniMax, Alibaba, Moonshot), workspace/
    deployment/project ID (Azure OpenAI, OpenAI project scoping, OpenCode Go), enterprise/base-URL override
    (Azure, LLM Proxy, LiteLLM, ClawRouter, sub2api, Wayfinder).
  - **Multi-account / token accounts**: add/label/remove multiple credentials per provider (e.g. several Claude or
    Codex logins), each with an "add account" / "switch account" / "remove" flow, active-account indicator.
  - Manual "Sign in" / OAuth / device-flow buttons where relevant (Codex, Claude, Cursor, Gemini login runners).
  - Inline **live preview** of that provider's current menu card (so settings show real fetched data, not just a
    form).
  - **Error display**: expandable error text + copy button, tied to that provider's last fetch failure.
  - **Manual refresh** button scoped to that provider.
  - Codex-specific: a dedicated **accounts section** listing every visible Codex account (managed + system/live),
    with reauthenticate / remove / set-active actions and status text.

## 6. Settings window structure (macOS "System Settings" style sidebar)

Fixed app-level panes above the per-provider list:

1. **General** — UI language (21-language picker), terminal app to open for session focus, launch-at-login toggle,
   global refresh interval (Adaptive default, or manual/1m/2m/5m/15m/30m fixed), "refresh all on menu open" toggle,
   "check provider status" toggle, global open-menu keyboard shortcut recorder, Quit button.
2. **Usage & Spend** — persistent local cost/spend dashboard (see §9).
3. **Notifications** — quota-depleted alerts, threshold warnings, predictive pace warnings, confetti (see §10).
4. **Menu Bar** — icon style, high-contrast toggle, menu-bar layout token editor, Merge Icons + switcher-row count
   + "show most-used provider" + Overview-tab provider picker (max 6), "surprise me" animation toggle.
5. **Menu** — usage-bar fill style, quota-warning markers on/off, weekly pace "work days" picker, reset-time
   display style (countdown vs. absolute clock), provider changelog links toggle, "show credits & extra usage"
   toggle, multi-account layout picker (switcher bar vs. stacked cards), cost-summary settings (which window,
   history-days stepper 1–365, cost-comparison-periods toggle, per-provider cost fetch status footer), Agent
   Sessions toggle + label-style picker + manual SSH-hosts field (see §11).
6. **Advanced** — Install CLI button (symlinks the bundled CLI into PATH), "hide personal info" (redacts
   account/session identifiers from menu + hook payloads), "disable keychain access" (falls back to config-file
   secrets only), "show provider storage usage" toggle, "show debug settings" toggle (reveals a Debug pane).
7. **Hooks** — local automation rule editor (see §12).
8. **About** — standard about panel: version/build, credits, links (GitHub, website, Twitter, email).
9. **Debug** (hidden unless enabled in Advanced) — developer-facing diagnostics pane.
10. Per-provider settings row for each of the 63 providers (see §5).

## 7. Usage & Spend dashboard

A dedicated, persistent view (not just a menu popover) showing local cost history:

- Covers providers that expose token-cost history: Codex, Claude, Vertex AI, OpenAI, Mistral, AWS Bedrock (others
  omitted entirely rather than shown empty).
- **Time range picker**: 7d / 30d segmented control.
- Grouped **by native currency** — never sums/converts across currencies.
- Per-currency: total spend, subscription/provider ranking, daily chart (bar/line), top-model breakdown.
- **Coverage indicator**: "X / Y days covered" — flags when the scan window is shorter than the requested range
  instead of silently claiming full coverage.
- Manual refresh button + spinner; keeps last-good data if a refresh fails.
- Refresh-failure counter shown in a footer.
- **Share action** — renders a shareable image/card summarizing usage stats (provider, subscription, tokens, cost,
  per-model breakdown) for posting/sharing outside the app.
- Auto-refreshes on day change, timezone change, and app-active.

## 8. Notifications & alerts

- **Quota-depleted notification** — session quota crosses into full depletion.
- **Threshold warning notifications** — configurable per-window (session/weekly) percentage thresholds; crossing
  one upward fires a notification. Global default thresholds, overridable per-provider.
- **Predictive pace warnings** — proactive "you're on track to run out before reset" alert based on pace
  calculation, independent of a hard threshold being crossed.
- Delivery controls: system notification sound on/off, on-screen alert overlay on/off (in addition to/instead of
  the OS notification center entry).
- **In-menu quota warning markers** — small visual tick/marker on the usage bar itself at the warning threshold.
- **Weekly-reset confetti** — celebratory on-screen animation when a tracked window resets, with an
  off/on(/frequency) option.
- **Notification Center** entries the OS itself renders (standard macOS UserNotifications).

## 9. Local automation hooks

- Opt-in (`hooks.enabled`, off by default) rule engine, editable in Settings → Hooks or directly in the config
  file. No remote/HTTP trigger path — everything is local and explicit.
- Rule = event + optional provider/threshold filter + an **executable to run** (absolute path, argv list, timeout).
  Runs as a direct process invocation (never through a shell).
- Events: `quota_low` (threshold crossed), `quota_reached` (session fully depleted), `quota_reset`, 
  `provider_unavailable` / `provider_recovered` (status transitions), `refresh_failed`.
- Payload delivered both as `CODEXBAR_*` env vars and as JSON on stdin (event, provider, timestamps, usage %,
  used/limit, reset time, status).
- Failure containment: hook errors never block a provider refresh; repeated failure events are coalesced
  (10-minute window per provider/account/window) so a flapping provider can't hammer the hook command.
- Safety caps: max 32 rules, 32 args/rule, size limits on strings/payloads — configs beyond that fail closed.
- CLI mirrors this: `codexbar hooks list|enable|disable|test`.

## 10. Agent Sessions (live session list)

- Lists **currently running** Codex / Claude Code sessions — locally, and on other hosts reachable over SSH
  (Tailscale peer auto-discovery + a manual comma-separated SSH host list).
- Local sessions poll every 30s; remote every 60s (and on menu open).
- Row label modes: **Project** (working-dir name, default/most private), **Descriptive** (thread title/subagent
  task name, falls back to project), **Descriptive + project** (both, when they differ).
- Visual grouping: local sessions first, then per remote host; filled dot = active session, empty dot = idle.
- Clicking a row **activates** that session's terminal/editor/desktop app window (local: direct window focus,
  possibly prompting for Accessibility permission the first time; remote: same focus command run over SSH).
- CLI equivalent: `codexbar sessions`, `codexbar sessions --json`, `codexbar sessions focus <id>`.

## 11. Provider status / incident polling

- Toggle in Settings → General: "Check provider status".
- Sources: Statuspage.io feeds (OpenAI, Anthropic/Claude, Cursor, Factory, Copilot/GitHub) and the Google
  Workspace incidents feed (Gemini/Antigravity).
- Surfaces: a status-indicator dot in the provider settings sidebar, an incident summary line + freshness in the
  menu card, and an overlay glyph on the bar icon itself.
- Clicking the status line opens the provider's status page (or, for link-only providers with no pollable API,
  just opens the page without live polling).

## 12. Widgets (macOS WidgetKit — desktop/notification-center widgets, not applicable to Linux verbatim but useful
as a feature-parity reference for what an applet's "at a glance" surfaces could cover)

Six widget families, each reading a shared JSON usage snapshot the main app writes after every refresh:

- **Switcher** — static multi-provider switcher tile (small/medium/large).
- **Usage** — single configurable provider's usage (small/medium/large).
- **History** — usage-history chart for a configurable provider (medium/large).
- **Metric** — compact single-number tile: credits / cost-today / cost-30d (small only).
- **Burn Down** — session or weekly burn-down chart for a configurable provider (medium; Codex/Claude only).
- **Burn Down (Combined)** — session **and** weekly burn-down together (medium; Codex/Claude only).

Provider picker for configurable widgets currently covers: Codex, Claude, Cursor, Gemini, Alibaba, Antigravity,
z.ai, Copilot, MiniMax, Kilo, OpenCode, OpenCode Go (other providers appear in the raw snapshot but aren't yet
widget-selectable).

## 13. Bundled CLI (`codexbar`) — already Linux-native, this is the backend surface

Ships as a separate binary; this is the piece already usable headless on Linux (Homebrew/AUR/tarball installs),
and is what the applet would sit on top of.

- `codexbar` / `codexbar usage` — default command, text or `--format json` usage snapshot. `--provider <id|both|
  all>`, `--account <label>`, `--account-index <n>`, `--all-accounts`, `--source auto|web|cli|oauth|api`,
  `--status` (include status-page data), `--no-credits`, `--pretty`.
- `codexbar cost` — local token-cost usage (Claude/Codex from local session-log scans; Cursor from its cookie-
  authenticated dashboard, macOS-only). `--days N` (1–365), `--group-by project`, `--refresh` (bypass cache).
- `codexbar cards` — one-shot **terminal card grid** render of the usage snapshot (this is effectively a preview
  of "what the applet's popover should show"): provider/usage/reset per card, account + plan badge lines, a
  `--brief` compact-table mode, footer summary of any failed providers, enhanced true-color rendering in capable
  terminals.
- `codexbar serve` — foreground local HTTP JSON server: `GET /health`, `/usage`, `/usage?provider=…`, `/cost`,
  `/cost?provider=…`, and a token-gated `GET /dashboard/v1/snapshot`. Configurable host/port/refresh-interval/
  request-timeout; loopback-only by default, non-loopback binding requires an explicit bearer token + an explicit
  "yes I know this is cleartext" flag. This is the obvious integration point for a COSMIC applet to poll instead
  of shelling out per refresh.
- `codexbar guard` — CI/automation gate: exits non-zero (with stable, documented exit codes) if a provider's
  remaining quota is below a threshold. `--min-remaining`, `--window session|weekly`, `--fail-open`, `--json`.
- `codexbar config providers|enable|disable|set-api-key|validate|dump` — scriptable equivalent of the Providers
  settings pane.
- `codexbar cache clear` — clear cached cookies and/or cost scans.
- `codexbar cookie refresh` — force re-import of browser cookies for a provider.
- `codexbar hooks list|enable|disable|test` — scriptable equivalent of the Hooks settings pane.
- `codexbar sessions [--json] [focus <id>]` — Agent Sessions equivalent.

## 14. Configuration & data model (contract the applet must speak, not architecture)

- Single JSON config file (`~/.config/codexbar/config.json` by default; `XDG_CONFIG_HOME` and `CODEXBAR_CONFIG`
  overrides respected; legacy `~/.codexbar/config.json` path still read if present), `0600` permissions.
- Holds: per-provider `enabled`, `source`, `cookieSource`/`cookieHeader`, `apiKey`, `enterpriseHost`, `region`,
  `workspaceID`, `tokenAccounts`, plus the `hooks` block. Provider **display order** in the app/CLI is literally
  the array order in this file.
- The app and CLI **read the same file** — anything set in one surface is immediately visible in the other. This
  is the natural sync point for a COSMIC applet: read/write the same config file (or drive it via `codexbar
  config` subcommands) rather than inventing a parallel settings store.

## 15. Misc cross-cutting features

- **Localization**: full app + website UI in 21 languages, automatic locale detection, persistent per-user
  language override, RTL layout support (Arabic, Persian).
- **Privacy controls**: "Hide personal info" redacts account identifiers app-wide (menu, hook payloads); optional
  full Keychain-access kill-switch; on-device parsing by default, browser-cookie reads are opt-in and never store
  passwords.
- **Update mechanism**: Sparkle-based auto-update with a channel selector (stable/beta) — macOS-specific, but the
  *concept* of "update channel" is a UX feature worth deciding on for the applet/distro-packaging story.
- **Keyboard shortcut**: global hotkey to open/close the tracked menu.
- **Launch at login** toggle.
- **Terminal app selector** — which terminal emulator to focus when activating an Agent Session.

---

## Suggested next step

Once you're ready to move from "what features exist" to "what do we build," the natural groupings for a COSMIC
applet's own feature/view plan are roughly: (a) panel icon + popover usage card [§2–3], (b) provider management
[§5], (c) settings/preferences window [§6], (d) usage/spend history view [§7], (e) notifications [§8], (f) agent
sessions [§10], and (g) the CLI/`serve` integration layer [§13–14] that everything else is built on. Widgets
[§12] and Sparkle-style updates [§15] are the two sections least likely to have a 1:1 COSMIC equivalent and may
need a deliberate "won't port" or "port differently" decision later.
