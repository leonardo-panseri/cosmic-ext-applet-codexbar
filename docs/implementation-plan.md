# COSMIC applet implementation plan

Target: port [macos-app-features.md](macos-app-features.md) to a COSMIC panel applet, on top of the
`codexbar` CLI already installed at `/usr/bin/codexbar` (v0.145.0 payloads verified on this machine).

---

## 0. Shaping decisions

These six decisions remove most of the work before it is written. Each was verified against the CLI or
libcosmic, not assumed.

### D1 — Backend is `codexbar` subprocess calls, not `codexbar serve`

`tokio::process::Command` + `serde_json`. No HTTP client, no daemon to supervise/restart/port-allocate, no
token handling. Refresh is every 1–30 minutes; a ~1s process spawn is invisible at that cadence.

Verified payloads:

| Call | Feeds |
|---|---|
| `codexbar usage --provider all --format json --status` | §2, §3, §4, §11 — one call returns usage windows, pace, credits, identity, plan, **and** status-page data |
| `codexbar cost --days N --json` | §7 |
| `codexbar sessions --json` | §10 |
| `codexbar config providers\|dump\|enable\|disable\|set-api-key --json` | §5 |
| `codexbar hooks list\|enable\|disable\|test` | §9 |

Reconsider `serve` only if a single `usage --provider all` refresh measurably exceeds ~3s wall clock.

### D2 — One panel icon. Merge Icons is the *only* mode

macOS allows N status items per app; the COSMIC panel places an applet once by applet ID. "One icon per
provider" (§2) has no clean COSMIC equivalent — running N copies of the binary would need N applet IDs and N
desktop entries. So §4's Merge Icons switcher becomes the baseline UI, not a toggle. The `merge_icons`
setting itself disappears.

### D3 — Provider config lives in `~/.config/codexbar/config.json`, applet prefs live in cosmic-config

Two stores, no sync layer, matching §14. Never write `config.json` directly — drive it through
`codexbar config …`, which owns validation and the `0600` mode. The applet's own `Config` (cosmic-config,
already scaffolded in `src/config.rs`) holds only what codexbar has no field for: refresh interval, icon
style, reset-display style, thresholds, enabled panes.

### D4 — Popup stays small; a second window carries Settings and Usage & Spend

The `multi-window` feature is already enabled in `Cargo.toml`. Popup = the 372px-wide provider card (§3) plus
the switcher (§4). Settings (§6) and the spend dashboard (§7) open a normal `cosmic::app` window with a
`nav_bar` sidebar — which is exactly the macOS System Settings shape §6 describes, and is what libcosmic's
nav model is for.

### D5 — No new dependency that isn't already in `Cargo.lock`

`serde`, `serde_json`, `zbus` (→ notifications, §8), `jiff` (→ RFC3339 reset countdowns), `url` are all
already in the tree as transitives. Promote them to direct deps; compile cost is zero. Charts (§7) are
`container`s with proportional width in a row — no plotting crate.

### D6 — Won't port

| Feature | Reason |
|---|---|
| §12 Widgets (6 families) | COSMIC has no widget/WidgetKit surface. |
| §15 Sparkle auto-update + channel selector | Distro packaging (AUR/COPR) owns this. |
| §10 session *focus* | `codexbar sessions focus` is macOS-only (confirmed in `--help`). Linux needs cosmic-toplevel/wlr protocol work. Substitute: row click opens the configured terminal at the session `cwd`. |
| §2 drag-and-drop token layout editor | Ship 3–4 fixed presets instead. Add the composer only if presets prove insufficient. |
| §8 confetti, §2 "surprise me" | Easter eggs. Free to add later, zero value now. |
| §15 21 languages | Fluent/i18n-embed is already wired; adding *keys* is in scope, adding *translations* is a translator task. |
| §5 63 hand-written provider forms | See P4 — one generic form driven by `config dump`. |

---

## 1. File layout

Grow into this; do not create empty files up front.

```
src/
  main.rs        (unchanged)
  i18n.rs        (unchanged)
  config.rs      applet prefs — replace `demo: String`
  codexbar.rs    P0: CLI wrapper — types + async calls          (~300 lines)
  app.rs         AppModel, Message, panel view, popup view
  settings.rs    P4: settings window (nav model + panes)
  spend.rs       P6: Usage & Spend pane + bar chart
```

Split view code out of `app.rs` only when it passes ~600 lines.

---

## 2. Phases

Each phase is independently shippable and ends with a runnable check.

### P0 — Backend layer (`src/codexbar.rs`) — §13, §14

Serde structs for the payloads above. Model only fields the UI reads; `#[serde(default)]` everywhere, since
provider payloads are ragged (`primary` is `null` for Codex, `secondary` is null for others).

```rust
pub struct Snapshot { provider: String, source: String, usage: Usage, pace: Pace, credits: Option<Credits>, status: Option<Status>, error: Option<String> }
pub struct Usage { identity: Identity, primary: Option<Window>, secondary: Option<Window>, tertiary: Option<Window>, updated_at: Timestamp }
pub struct Window { used_percent: f32, resets_at: Option<Timestamp>, reset_description: Option<String>, window_minutes: u32 }
```

One generic runner: `async fn run_json<T: DeserializeOwned>(args: &[&str]) -> Result<T, Error>`, mapping
non-zero exit + stderr into a displayable error string (§3 error banner).

**Check:** `cargo test` with one `#[tokio::test]` asserting `usage --provider all` deserializes and the
`secondary.used_percent` round-trips. Skips itself if `codexbar` is absent.

### P1 — Panel icon + provider card — §2, §3

- Panel button: brand icon + percent text via `core.applet.icon_button` / `applet.text`, `applet::style()`.
  Icon styles from D6: `IconOnly` / `IconPercent` / `IconLabel`.
- Dim + overlay glyph when the last refresh errored (§2).
- Popup card: header (provider, account email, plan badge from `loginMethod`) → one row per non-null window
  with `widget::progress_bar` + % + countdown → pace line (already computed by the CLI: `pace.secondary.summary`
  is literally *"12% in reserve | Expected 45% used | Lasts until reset"*) → credits row → status line →
  "Open dashboard" row → collapsible error banner with copy button.
- Countdown vs. absolute clock toggle: `jiff` diff against `resets_at`, or the CLI's own `resetDescription`.

Pace (§3) needs no implementation — the CLI ships `stage`, `deltaPercent`, `summary`, `willLastToReset`.
The "work days" setting is a codexbar-side concern; expose it only if `config dump` shows a field for it.

**Check:** `just run` on the panel, card renders live account data.

### P2 — Switcher + Overview — §4

`segmented_button` for provider tabs, plus an Overview tab rendering up to 6 compact rows. Hide the tab when
nothing is chosen. "Show most-used provider" = `max_by used_percent` on load. Provider order = array order
from `config providers` (§14).

### P3 — Refresh engine — §2, §3

- `Subscription` on a `tokio::time::interval`, period from applet config (Adaptive / 1m / 2m / 5m / 15m / 30m).
  "Adaptive" = clamp to the nearest reset time, floor 1 minute.
- Refresh on popup open, if "refresh all on menu open" is set.
- In-flight dedup: a `refreshing: bool` guard in the model — repeated clicks return `Task::none()`. This is
  §3's "shared/deduplicated requests" without a request-coalescing layer.
- Spinner in place of the value; cap it so a hung provider can't spin forever (`tokio::time::timeout`, 30s).
- Keep last-good data on failure; show error inline.

**Check:** kill network, confirm stale-but-visible data + error banner, no spinner stuck.

### P4 — Settings window — §5, §6

One `cosmic::app` window, `nav_bar` sidebar. Panes:

1. **General** — refresh interval, terminal app, "refresh all on menu open", "check provider status", language override, launch-at-login (write a `~/.config/autostart/*.desktop`), Quit.
2. **Menu Bar** — icon style, high contrast, layout preset (D6), switcher-row count, Overview picker (max 6).
3. **Menu** — usage-bar fill style, warning markers, reset-display style, credits/extras toggle, cost-summary settings.
4. **Notifications** — see P5.
5. **Advanced** — hide personal info, show storage usage, debug pane toggle.
6. **Hooks** — see P5.
7. **About** — version, links.
8. **Providers** — *one* searchable list of the 63 from `config providers --json`, each with an enable toggle
   (`config enable|disable`) and a status dot. Selecting one opens a **generic** detail pane whose fields are
   whatever `config dump` exposes for that provider — API key, source picker, region, host, workspace ID,
   token accounts. §5 describes 63 bespoke forms; one data-driven form covers them because the config schema
   is uniform. Provider-specific login runners (Codex/Claude/Cursor OAuth) get a "Sign in" button that shells
   out to the provider's own CLI.

The inline live preview (§5) is the P1 card widget reused — the reason P1's card is a standalone function.

### P5 — Notifications + hooks — §8, §9

Both fire off the same event source: a diff of the previous vs. current snapshot in the refresh handler.

Events: `quota_low` (crossed a configured threshold upward), `quota_reached`, `quota_reset` (`resets_at`
moved), `provider_unavailable` / `provider_recovered` (status `indicator` changed), `refresh_failed`.

- **Notifications**: `org.freedesktop.Notifications.Notify` over `zbus`. Per-window thresholds, global
  defaults overridable per provider. Predictive pace warning = `pace.stage == farAhead && !willLastToReset`.
  Sound on/off is a Notify hint. In-bar warning markers are a tick in the P1 progress row.
- **Hooks**: `hooks list` shows the rules; `hooks enable|disable|test` wires the buttons. Rule *editing* stays
  in `config.json` for now — a 32-rule argv editor is a lot of UI for an off-by-default feature. Firing the
  configured executable is ours (`tokio::process`, never a shell, `CODEXBAR_*` env + JSON on stdin, per-event
  10-minute coalescing window, honor the 32-rule / 32-arg caps and fail closed past them).

**Check:** one test asserting the event diff — threshold crossed upward fires once, staying above does not
re-fire, and dropping below re-arms.

### P6 — Usage & Spend — §7

Pane in the settings window (D4). `cost --days N --json` already returns `daily[]`, `modelBreakdowns[]`,
`projects[]`, `currencyCode`, and `historyDays` — the coverage indicator is `daily.len()` vs. requested days.

7d/30d `segmented_button`; group by `currencyCode`, never sum across currencies; per-currency total, provider
ranking, daily bar chart (proportional `container`s), top-model table. Manual refresh keeps last-good data.
Auto-refresh on day change. **Skip the share-image renderer** (§7) — no offscreen-render path in libcosmic
worth building for a screenshot.

### P7 — Agent Sessions — §10

`sessions --json` gives `host`, `projectName`, `state`, `provider`, `cwd`, `startedAt`. Popup section: local
first then per-host, filled/empty dot for active/idle, three label modes (Project / Descriptive /
Descriptive+project). Poll 30s local while the popup is open. Row click launches the configured terminal at
`cwd` (D6 — no window focus).

### P8 — Polish — §15

Hide personal info (redact email/session identifiers at the render layer, and in hook payloads), storage
usage submenu, global open-menu shortcut (a cosmic-settings custom shortcut invoking the binary with a flag —
COSMIC has no in-app global hotkey API), provider changelog links.

---

## 3. Feature coverage

| § | Feature | Phase |
|---|---|---|
| 1 | Core concept | — |
| 2 | Panel icon, meter, styles, layout presets | P1, P3 |
| 3 | Provider card, usage rows, pace, credits, cost, errors | P1 |
| 4 | Merge icons / switcher / overview | D2, P2 |
| 5 | Provider management (63) | P4 |
| 6 | Settings window (8 panes) | P4 |
| 7 | Usage & Spend dashboard | P6 |
| 8 | Notifications & alerts | P5 |
| 9 | Local automation hooks | P5 |
| 10 | Agent Sessions | P7 |
| 11 | Provider status polling | P1 (free via `--status`) |
| 12 | Widgets | won't port |
| 13 | CLI backend | P0 |
| 14 | Config/data model | D3, P0 |
| 15 | Misc cross-cutting | P8, partial won't-port |

---

## 4. Order

P0 → P1 → P3 → P2 → P4 → P5 → P6 → P7 → P8.

P1+P3 alone is a useful applet. Everything after that is additive.
