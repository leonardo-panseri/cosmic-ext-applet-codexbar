# Rewrite plan: COSMIC adapter for CodexBar for Linux

This plan replaces the plan for the one-day prototype, which called the `codexbar` CLI directly.
That plan is still in git history (`6f88924:docs/implementation-plan.md`).

**Goal.** Rebuild the applet as a display-only adapter for the upstream Linux desktop app, `codexbar-linux`,
in the same way as upstream's Omarchy widget. Ship it as a Flatpak in the same form as other COSMIC
applets. When the desktop app is not reachable, the applet tells the user to install it by following the
[official instructions](https://github.com/steipete/CodexBar/blob/main/Integrations/Linux/README.md#install-release-archives)
and to leave **Start at login** enabled.

**Upstream reference.** steipete/CodexBar `1f6da9bec` (2026-11-14), app version 0.84.1, snapshot
`schemaVersion` 1. Source files: `Integrations/Linux/{main.cpp,DesktopController.cpp,Shared/Usage.js}`
and `Integrations/Omarchy/Panel.qml`.

---

## 1. Decisions

### D1 — The desktop app is the only data source

The applet never runs `codexbar`, never reads provider credentials and never scans cost logs. The
desktop app already owns these:

- provider polling, its timeouts and output limits
- provider-specific window logic, such as Antigravity families, scoped caps and Zed/Kiro context windows
- notifications
- local spending scans
- the bar text, reset text, warning flags, and the used/remaining preference

All of it reaches us already computed, as snapshot fields.

Consequences:

- Each panel and popup update is one socket round trip, about 0.02 ms in a local benchmark against a fake server.
- Notifications are never sent twice, and providers are never polled twice.
- **Dropped compared with the prototype:** agent sessions, hooks, provider API key editing, the applet's own
  settings window, and the Usage & Spend dashboard. The dashboard and settings are now the desktop
  app's Qt windows, which the applet opens over IPC.

### D2 — Talk to the socket directly, not through `codexbar-linux --snapshot`

`--snapshot` sends `{"command":"snapshot"}` to the socket and copies the reply to stdout unchanged
(`main.cpp`, `request()`), so it gives no protection from protocol changes. Going through it would mean:

- **Flatpak:** a host-escape permission, `--talk-name=org.freedesktop.Flatpak`, so we could run `flatpak-spawn --host`.
- **Cost:** a Qt process every 5 s, measured at about 11 ms CPU and 24 MB peak RSS with a stand-in client that links less Qt
  than the real binary.

The socket needs only `--filesystem=xdg-run/codexbar-linux:create`.

Upstream documents the socket for adapters under "Adapter interface". Adapters must check `schemaVersion`,
accept unknown fields, and treat a missing backend as unavailable. We do all three.

### D3 — No applet settings

Upstream already keeps every display preference in the desktop app's Settings window, the Omarchy widget
reads them through the snapshot, and the two adapters should look the same. That covers:

- providers and their order
- used or remaining display, and the reset format
- `showBarDetail`, `showScopedCaps` and `barProviders`
- warning colours, and pace

So the applet has no settings. The `config.rs`, cosmic-config schema and settings window all go. If
an applet-only preference ever becomes necessary, add it then.

The one exception is internal state with no settings UI: a single cosmic-config flag,
`tray_prompt_dismissed`, so the tray prompt (D8) appears only once.

### D4 — We can't tell "not installed" apart from "not running"

The sandbox can't see the host `PATH`, and it can't start the desktop app without a host-escape
permission. So "unreachable" is a single state with one message covering both cases. The message
links to the official installation guide and tells the user to open CodexBar from the launcher and keep
**Start at login** on.

The one distinction we can make for free is between "socket file present but connection refused" and
"no socket". We use it only to choose the wording (D7).

### D5 — App ID `io.github.leonardo_panseri.cosmic-ext-applet-codexbar`

Flatpak IDs allow a hyphen only in the last part, so the current
`com.github.leonardo-panseri.…` is not a valid Flatpak ID. Flathub's convention for GitHub-hosted apps
is `io.github.<user>` with `-` → `_`.

Our existing panel entry must be removed and added again once. Nobody else uses the prototype.

The crate and binary stay `cosmic-ext-applet-codexbar`. That name is also andrew-verde's crate, but
crate names collide only on crates.io and the AUR, and we publish to neither. Our Flatpak ID and our
display name are distinct.

### D6 — Vendor the provider logos

The sandbox can't read the host install, so we copy upstream's `Sources/CodexBar/Resources/ProviderIcon-*.svg`
(110 files, MIT) into `resources/providers/` and embed them with `rust-embed`, which is already a
dependency. We draw them as symbolic icons so they take the panel's text colour, as Omarchy does. A
provider without a logo shows the snapshot's `tag` text, as Omarchy does too.

The logos are refreshed by `just sync-icons`, which copies them from a local upstream checkout, plus a
third-party notice.

### D7 — What the user sees in each backend state

| State | Detected by | Panel | Popup |
|---|---|---|---|
| `Connecting` | Startup: the first 10 s without a successful snapshot | App icon | "Connecting to CodexBar…" |
| `Unavailable` | Connect fails after the 10 s grace period | App icon with a warning badge | Install/run guidance (§4.4) |
| `Incompatible` | `schemaVersion` ≠ 1, or the reply isn't valid JSON | App icon with a warning badge | "This CodexBar version isn't supported. Update CodexBar and this applet." plus links |
| `Connected` | Valid snapshot | `barEntries` (§4.2) | Usage (§4.3) |
| `Connected` with autostart off | `{"command":"autostart","action":"status"}` returns `enabled:false` | Unchanged | Usage plus a banner: "Start at login is off…" with a **Turn on** button |

The 10 s grace period exists because at login the applet and the desktop app start at the same time,
and without it the warning would flash on every login.

### D8 — Ask the user to hide the desktop app's tray icon

The desktop app shows its own tray icon by default (`showTray: true`). COSMIC's status area shows it next to our
applet, so the same data appears twice. Upstream's Omarchy installer turns it off for the same reason
(`install.py` writes `showTray = False` under `--omarchy`).

What the socket allows:

- **We can turn it off.** `{"command":"configure","settings":{"showTray":false}}` merges into the current settings,
  validates them, saves `~/.config/codexbar/linux.json` atomically and hides the tray at once. It replies
  `{"ok":true}`, or `{"ok":false,"error":…}`, for example when `linux.json` is invalid.
  `codexbar-linux --configure '{"showTray":false}'` does the same from a terminal.
- **We can't read whether it's on.** No IPC command returns settings, and the snapshot doesn't include
  `showTray`. Reading `linux.json` would need a filesystem permission on `~/.config/codexbar/`, next to the
  CLI's `config.json` with provider API keys, and we won't ask for that.

So:

- **README and guidance (§4.4)** tell the user to turn off **Tray icon** in CodexBar Settings → General and press
  **Save**.
- **One-time popup banner** in the `Connected` state: "CodexBar's tray icon shows the same usage as this applet."
  It has two buttons:
  - **Hide tray icon** sends the `configure` request above and shows its `error` if `ok` is false.
  - **Keep it** dismisses the banner.

  Either button sets `tray_prompt_dismissed`, the flag from D3. The applet never changes the setting without the click.

---

## 2. The protocol we depend on

These are facts read from upstream source, not guesses.

**Socket path.** `$XDG_RUNTIME_DIR/codexbar-linux/desktop.sock`. Upstream falls back to
`/tmp/codexbar-<uid>/codexbar-linux` when `XDG_RUNTIME_DIR` is empty. The sandbox can't reach that
fallback, so we treat that case as `Unavailable`.

**Framing.** One request per connection. The request is a single line of compact JSON ending in `\n`.
The reply is a single line of JSON, after which the server closes the connection.

**Server limits.** Requests may be at most 64 KiB, and the server closes the connection after 3 s.
The client in `main.cpp` uses a 500 ms connect timeout, a 3 s total deadline and a 4 MiB reply cap.
We use the same values.

**Commands we use:**

| Request | Reply | Used for |
|---|---|---|
| `{"command":"snapshot"}` | snapshot | Every 5 s poll |
| `{"command":"snapshot","spending":true}` | snapshot; also starts a spending scan if its cache is older than 5 minutes | When the popup opens |
| `{"command":"refresh"}` | `{"ok":true}` | Refresh button, middle click |
| `{"command":"usage"}`, `"spending"`, `"settings"` | `{"ok":true}` (the desktop app opens its window) | Popup footer |
| `{"command":"autostart","action":"status"}` | `{"ok":true,"enabled":bool}` | On connect, and when the popup opens |
| `{"command":"autostart","action":"enable"}` | `{"ok":bool,"enabled":bool}` | **Turn on** button |
| `{"command":"configure","settings":{"showTray":false}}` | `{"ok":bool,"error":String}` | **Hide tray icon** button (D8) |

`configure` is sent only with `showTray: false`, and only after the user clicks. We never send `quit`.

**Snapshot v1.** We model only the fields we draw, and every field is `#[serde(default)]`:

```text
schemaVersion: 1          summary: String            quotaDisplay: "remaining"|"used"
busy, stale: bool         error: String              updated: "HH:mm" | ""
barEntries: [{provider, tag, text}]        // missing on older backends: fall back to `summary`
entries: [{
  provider, error, plan, status, statusLevel, updatedAt,
  credits: f64?, extraUsage: {period, currency, used?, limit?, balance?}?,
  resetCredits: {available, nextExpiresAt}?,
  windows: [{key, label, minutes?, remaining, displayValue, displaySuffix, warning,
             resetText, pace, paceExpected?, paceDelta?, scoped?}]
}]
costBusy: bool  costError: String
spending: [{provider, today?, month?, tokens?, provenance, error}]
```

Upstream's snapshot excludes account identity, so the applet has no privacy logic.

The backend fills in `resetText` when the snapshot is taken. Polling every 5 s keeps countdowns
current, so we never parse timestamps, and `jiff` is no longer needed.

---

## 3. File layout

Start from an empty `src/`. Everything below is new. Only `i18n.rs` carries over unchanged.

```
src/
  main.rs        entry point (from the template)
  i18n.rs        unchanged
  backend.rs     socket client: path resolution, request/reply framing, limits, typed errors   (~150 lines)
  snapshot.rs    serde types for §2 and a schemaVersion gate                                    (~120 lines)
  app.rs         AppModel, Message, poll subscription, the D7 state machine, panel button        (~350 lines)
  popup.rs       popup view: status line, provider cards, guidance, footer                       (~350 lines)
  icons.rs       embedded provider SVGs, with the tag as fallback                               (~60 lines)
resources/
  app.desktop, app.metainfo.xml, icon.svg, providers/ProviderIcon-*.svg, THIRD_PARTY_NOTICES.md
flatpak/
  io.github.leonardo_panseri.cosmic-ext-applet-codexbar.json, cargo-sources.json
tests/fixtures/  snapshot-*.json captured from a real backend (§5 P0)
docs/adr/0001-adapter-over-codexbar-linux.md
CONTEXT.md       glossary: desktop app/backend, snapshot, bar entry, window, adapter
```

That's about 1,100 lines, against about 6,000 in the prototype.

**Done ahead of P1:** the prototype is tagged `prototype-cli` (`6f88924`). The deletions and rewrites below have been
made. `src/` now holds a minimal applet with the new app ID that opens a placeholder popup. The `Cargo.toml` changes are
also done.

**Delete:**

- `src/{alerts,codexbar,config,privacy,sessions,settings,spend,popup}.rs`
- the current `app.rs`
- `docs/macos-app-features.md`: the desktop app now owns parity with the Mac app

**Rewrite:**

- `i18n/en/cosmic_ext_applet_codexbar.ftl` (154 lines of prototype strings): keep only the keys the new views use.
- `README.md`: remove the `CODEXBAR_LIVE_TEST` section and the vendoring/distro packaging section. Add the requirement for
  CodexBar for Linux and the Flatpak install steps.
- `justfile`, `resources/app.desktop`, `resources/app.metainfo.xml`: change to the D5 app ID. The icon file is installed
  under the app ID's name.

**Cargo changes:**

- **Remove:** `jiff`, `url`, `zbus`.
- **Keep:** `serde`, `serde_json`, `rust-embed`, `i18n-embed`, `i18n-embed-fl`.
- **tokio:** `["rt", "net", "time", "io-util", "macros"]` instead of `full`.
- **libcosmic:** keep the template's features, including `multi-window`, which the current template still enables
  for applets.
- **Template sync (Nov 2026):** desktop entry and metainfo are generated by `xdgen` (`build.rs` → `target/xdgen/`), as in
  pop-os/cosmic-applet-template `58f506f`. The README follows the COSMIC trademark policy and describes the applet as
  "for the COSMIC™ desktop".

---

## 4. Behaviour

### 4.1 Polling

- **Subscription:** a tokio interval every 5 s sends `snapshot`, matching Omarchy's cadence. There is at most one request in
  flight; a tick that finds one running is skipped.
- **Popup opens:** send one `snapshot` with `"spending":true` right away, plus `autostart status`.
- **After Refresh:** poll again right away, and keep 1 s polls while `busy` is true, capped at 60 s, which is the backend's own
  query deadline.
- **When `Unavailable`:** keep the same 5 s cadence, since a failed connect costs microseconds. This also means the applet
  recovers on its own when the desktop app starts.

### 4.2 Panel button

**Horizontal panel:** for each entry in `barEntries`, the provider logo (or `tag`) followed by `text`, separated by
`·`. Then:

- add `+N` when `entries.len()` is larger than `barEntries.len()`;
- put a leading `!` before everything when `stale` is true;
- with no `barEntries`, show the `summary` string;
- with an empty snapshot, show the app icon.

**Vertical panel:** the app icon only. Its badge shows the D7 state and staleness.

**Mouse:** left click toggles the popup, middle click sends `refresh`, matching Omarchy.

### 4.3 Popup

All `entries` go in one scrolling list, as in Omarchy.

1. **Status line:** `error`, or "Refreshing…" while `busy`, or "Updated {updated}", with "· older data" added when `stale`.
2. **One card per entry:**
   - **Header:** provider logo, display name and `plan`. The display name comes from a small id→name table copied from
     `Usage.js::providerName`, falling back to the capitalised id. Add a `statusLevel` dot and `status` text when the status
     isn't operational.
   - `error`, when set.
   - **Each window:** `label · {displayValue}% {displaySuffix}`, a `progress_bar` showing `displayValue` (destructive
     colour when `warning`), `resetText`, and `pace` when it isn't empty.
   - `credits`, `extraUsage` and `resetCredits` rows, each only when present.
   - Spending from the matching `spending` row: today and 30 days. Show `error` instead when it's set.
3. **Footer:** **Refresh**, **Usage & Spend…**, **Settings…**.

**Keyboard:** `R` refreshes and `Esc` closes.

### 4.4 Guidance (`Unavailable`)

> **CodexBar for Linux isn't running**
> This applet shows usage from the CodexBar desktop app. If it isn't installed, follow the official
> installation guide. If it is, open **CodexBar** from the app launcher. Keep **Start at login** on in
> CodexBar Settings so usage is available after you log in, and turn off **Tray icon** there, since this
> applet replaces it.
>
> [Open installation guide] [Check again]

When a socket file exists but the connection is refused, the first line becomes "CodexBar for Linux
isn't responding" and the text suggests restarting it.

Links open through `xdg-open`. Inside the Flatpak runtime, that is flatpak-xdg-utils, which goes through
the OpenURI portal, so no extra permission is needed.

### 4.5 Autostart banner

The banner reads: "Start at login is off. CodexBar won't be running after you next log in, and this
applet will be empty." Its **Turn on** button sends `autostart enable` and then checks the status again.

The applet never changes autostart without the user clicking. The banner disappears once `enabled` is true.

### 4.6 Localisation

Our own strings use Fluent (`i18n/en`). Labels, reset text, pace, plan and status come from the backend
in English. We show them as they are.

---

## 5. Phases

Each phase can be shipped on its own and ends with a check.

### P0 — Spike: remove the risks (half a day, throwaway branch)

1. **Install the desktop app.** Follow the README on the dev machine. It needs `qt6-declarative`, `qt6-svg` and
   `qt6-wayland` plus the CLI; this machine has only `qt6-base` today.
2. **Capture fixtures.** Save `codexbar-linux --snapshot` and `--snapshot --with-spending` output for these cases:
   - one provider
   - several providers, including one that errors
   - `quotaDisplay: used`
   - `showBarDetail` on

   Scrub them and save them to `tests/fixtures/`.
3. **Socket access from the sandbox.** Build the template applet as a Flatpak with
   `--filesystem=xdg-run/codexbar-linux:create` and confirm it connects in both orders: desktop app first, and applet
   first (where Flatpak creates the folder). Confirm that the desktop app accepts a folder Flatpak created: it checks
   ownership and that the folder isn't a symlink, then sets mode `0700`. Also confirm it survives a desktop app restart.
4. **Icon tint.** Confirm the upstream SVGs take the theme colour with libcosmic's `icon::from_svg_bytes(..).symbolic(true)`.
5. **App ID.** Confirm `flatpak build-init` accepts it.
6. **Panel registration.** Confirm cosmic-panel loads the Flatpak applet with its panel socket. COSMIC finds Flatpak
   applets by the exported launcher path (see andrew-verde's `docs/install.md`).
7. **Software renderer.** The libcosmic book suggests applets drop `wgpu` to use the software renderer and save memory.
   Build with `default-features = false` plus the features we need, and compare memory use and rendering in the real
   panel. Keep whichever works.

**Check:** a short note in the ADR covering each item. If item 3 fails, stop and rethink D2.

**Done 2026-11-16.** All items passed; see `docs/adr/0001-adapter-over-codexbar-linux.md`. The findings that
change later phases are noted in P2 and P5 below.

### P1 — Reset and backend client

- Write `snapshot.rs` and `backend.rs`. `backend::request(path, command) -> Result<Reply, BackendError>`, where
  `BackendError` is `NotRunning`, `NotResponding`, `Timeout`, `TooLarge` or `Protocol`.
- **Tests:**
  - every fixture parses;
  - unknown fields are ignored;
  - `schemaVersion: 2` → `Incompatible`;
  - a missing `barEntries` → fall back to `summary`;
  - against a fake tokio `UnixListener` in a temp folder: no socket, a refused connection, a server that never replies (the
    3 s deadline), an oversized reply, and a reply without a newline.
- Write the ADR and `CONTEXT.md`.

**Check:** `cargo test` and `just check` are clean.

**Done 2026-11-16.** 21 unit tests, plus an ignored read-only test against the real desktop app
(`cargo test -- --ignored live`). Differences from the description above:

- `backend::Backend` has one typed method per command (`snapshot`, `refresh`, `open`, `autostart_status`,
  `enable_autostart`, `hide_tray`) instead of a single `request(path, command) -> Reply`, so callers never
  match on a reply kind that can't occur.
- `BackendError::Protocol` is called `Incompatible`, after the D7 state it maps to. It covers invalid JSON and a
  `schemaVersion` other than 1.
- `BackendError::Rejected(error)` is new: the desktop app replied `ok: false`, for example `configure` with an
  invalid `linux.json`, or `Unknown command` from an older desktop app.
- A reply that ends without `\n` is `NotResponding`, not a protocol error. Upstream's client also treats it as
  no reply, and it only happens when the desktop app closes mid-reply.
- The §4.2 rules that depend only on the snapshot (bar entries and `+N`, the `summary` fallback, the empty case)
  live in `Snapshot::bar()`. Staleness is left to the panel view.
- The ADR was already written in P0; `CONTEXT.md` is new.

### P2 — Panel button and state machine

The D7 state machine, the 5 s subscription and the horizontal and vertical panel views from §4.2.

Decide two panel details here. The libcosmic book's power-applet example does both; the current template does
neither:

- Open the popup on mouse down (`on_press_down`) instead of on release (`on_press`).
- Add `X-OverflowPriority` to the desktop entry, which sets how the panel handles the applet when it runs out of room.

From P0:

- Wrap the panel view in `core.applet.autosize_window(..)`, or the panel clips the text to icon size.
- In `Cargo.toml`, set `default-features = false` on libcosmic, never enable `wgpu`, and drop `x11`. Decide on
  `a11y`, then check the build in the real panel.

**Check:** `just run` against the real desktop app. Stop the app with `codexbar-linux --quit`: after 5 s the panel
shows the warning badge, and when the app is started again it recovers on its own.

**Done 2026-11-16.** 15 new unit tests for the state machine, the poll schedule and the panel content, plus 4 for
the icons; the real-panel check is still to do. Decisions:

- **Press down.** The popup opens on `on_press_down`, like COSMIC's own applets and libcosmic's
  `applet.text_button`.
- **`X-OverflowPriority=10`** in `resources/app.desktop`, the value of most cosmic-applets. Without it
  cosmic-panel force-shrinks the applet when it runs out of room, which would clip the text; with it the applet
  moves to the overflow popup instead.
- **No `a11y`.** libcosmic's feature set is now exactly cosmic-applets': `default-features = false` with
  `applet`, `applet-token`, `dbus-config`, `multi-window`, `tokio`, `wayland` and `winit`. libcosmic routes an
  accessibility click only to `on_press`, never to `on_press_down`, so `a11y` would cost memory without making
  the button usable.
- **Logos.** 110 `ProviderIcon-*.svg` from upstream `1f6da9bec` in `resources/providers/`, embedded by
  `icons.rs`. Provider ids other than `[A-Za-z0-9_-]+` get no logo, so a snapshot can't name a path.
  `just sync-icons <checkout>` copies them and rewrites `resources/THIRD_PARTY_NOTICES.md` with the commit and
  upstream's licence.

Differences from the description above:

- The pure logic is `app::Connection` (D7 state plus poll schedule, driven with explicit `Instant`s) and
  `app::Panel` (what the button shows). The subscription ticks every 1 s only while fast polling, so the applet
  doesn't wake every second otherwise.
- A refresh that finishes while a poll is in flight queues another poll for when it returns, since the reply in
  flight may predate the refresh. A refresh the desktop app rejects, or that can't connect, polls once but doesn't
  start fast polling.
- `Incompatible` replies (also `TooLarge`, and `Rejected`) show at once, without waiting out the grace period:
  a reply came back, so waiting can't help. `Timeout` counts as "not responding".
- After the first snapshot there's no grace: one failed poll shows `Unavailable`.
- Badges are the `dialog-warning-symbolic` icon in the theme's warning colour for `Unavailable` and
  `Incompatible`, and `emblem-important-symbolic` in the text colour for a stale snapshot, at half the icon size
  in the bottom-right corner. An empty but stale snapshot shows the app icon with the stale badge.
- The placeholder popup now shows the D7 status line ("Connecting to CodexBar…", "isn't running", "isn't
  responding", the `Incompatible` text). P3 and P4 replace it.
- Backend methods for P3/P4 (`open`, `autostart_status`, `enable_autostart`, `hide_tray`) and the test-only
  `socket()` keep narrow `#[allow(dead_code)]`s with a comment.

### P3 — Popup

The status line, provider cards, footer actions and keyboard shortcuts from §4.3.

**Check:** the footer buttons open the desktop app's windows, and spending appears after opening the popup.

**Done 2026-11-16.** `src/popup.rs` draws the `Connected` popup; the other states keep P2's placeholder
until P4. 10 new unit tests for the formatting and shortcuts, against the fixtures, plus 2 for the
popup-open poll; the real-panel check is still to do. Decisions:

- **Formats** follow upstream's adapters where they have one: the status line and window line as in
  Omarchy's `Panel.qml`; `providerName` copied from `Usage.js`; `money` as `Usage.js` (`$3.25`, or
  "Unavailable"); credits as the desktop app's quick view ("Credits: 0 remaining"); spending as its quick
  view ("Today $3.25 · Last 30 days $42.50", or the row's `error`). No Linux adapter draws `extraUsage` or
  `resetCredits`, so these follow the Mac menu card: "{period}: {used} / {limit} · Balance {balance}" with
  only the amounts present, dollars for `USD` and otherwise numbers followed once by the currency
  ("Extra usage: 0 / 0 Credits"); and "Reset credits: 3 available".
- **Status dot:** warning colour for `minor` and `maintenance`, destructive for `major` and `critical`, text
  colour otherwise. A level without `status` text (`unknown` when the status page wasn't checked) shows
  nothing.
- **Popup open:** `Connection::popup_opened` marks the next snapshot request as `spending: true`. If a poll is
  in flight, it reuses the refresh path: another poll goes out as soon as that one returns, so there is still
  at most one request in flight. `autostart status` is sent alongside and its result kept in
  `AppModel::autostart` for P4. Plain polls keep the spending rows (the desktop app sends its cached scan in
  every snapshot), so spending stays after the first scan.
- **Footer:** Refresh is disabled while `busy`, as in Omarchy. **Usage & Spend…** and **Settings…** close the
  popup, as Omarchy's `launch()` does; a failed `open` is dropped, since the next poll shows `Unavailable`.
- **Keyboard:** `keyboard::listen` while the popup is open. `R` matches by key position on non-Latin layouts
  and ignores key repeat and Ctrl/Alt/Super; `Esc` closes.
- **Scrolling:** the card list scrolls past 520 px, so the popup stays on screen with many providers.

Differences from the description above:

- With an empty `updated` (the first seconds after the desktop app starts), the status line says "Waiting for
  usage…", as the desktop app's quick view does, instead of a bare "Updated".
- `resetCredits.nextExpiresAt` isn't shown: it is a UTC timestamp, and the applet doesn't parse timestamps
  (§2). Showing its date part could be a day off. If it matters, ask upstream for preformatted text.
- `backend::Window::Spending` stays unused: "Usage & Spend…" opens `usage`, as upstream's adapters do.

### P4 — Guidance and autostart

§4.4 and §4.5, the tray prompt from D8, and the `Incompatible` view.

**Check:** with the desktop app stopped, the popup shows the guidance and the link opens the README. After running
`codexbar-linux --autostart disable` the banner appears, **Turn on** fixes it, and
`codexbar-linux --autostart status` agrees. With the tray icon on, the tray prompt appears once. **Hide tray icon**
removes the icon from COSMIC's status area and sets `"showTray": false` in `linux.json`. The prompt doesn't come back
after a restart.

### P5 — Flatpak packaging

**Manifest** (`flatpak/…json`), in the cosmic-flatpak format:

- `runtime`: `org.freedesktop.Platform` 25.08, with `org.freedesktop.Sdk.Extension.rust-stable`
- `base`: `com.system76.Cosmic.BaseApp` `stable`
- `finish-args`:
  - `--socket=wayland`
  - `--share=ipc`
  - `--device=dri`
  - `--filesystem=xdg-config/cosmic:rw`
  - `--talk-name=com.system76.CosmicSettingsDaemon`
  - `--filesystem=xdg-run/codexbar-linux:create`

  There is no `--share=network` and no `org.freedesktop.Flatpak`.

**Other files:**

- `cargo-sources.json` generated by `flatpak-cargo-generator`.
- The desktop entry keeps `X-CosmicApplet=true`, `NoDisplay=true` and `X-CosmicHoverPopup=Auto`. Its `Exec` must
  never contain `%F` (already removed), because cosmic-panel can't inject its Flatpak arguments into a
  `--file-forwarding` command (P0 item 6).
- Metainfo: summary, screenshots, MPL-2.0, a note that the provider logos are MIT, and a description that names
  the host requirement. `appstreamcli compose` fails without the description. Fix the remote icon URL as well.
- For local builds, run `org.flatpak.Builder` with its state and build folders under `$HOME`, since its `/tmp` is
  private. The draft manifest from P0 is already in `flatpak/`. It builds the local `main` branch with network
  access during the build (`--share=network`); P5 replaces that with `cargo-sources.json`.

**justfile recipes:**

- `flatpak-build`
- `flatpak-install`
- `flatpak-sources`: regenerates `cargo-sources.json`
- `sync-icons`

**CI:** a GitHub Actions workflow running `cargo test`, clippy and a Flatpak build.

**Check:** install from a local repo, add the applet in Panel settings, and run P2–P4 again inside the sandbox.

### P6 — Publish

Open a PR to pop-os/cosmic-flatpak adding `app/<id>/<id>.json` and `cargo-sources.json`, pinned to a tagged commit
of this repo. Later releases are PRs that bump that commit.

Flathub's requirements reject apps that "rely on host components … for core functionality", and
cosmic-flatpak exists for applets that don't fit Flathub. The PR description should explain this, and the
README should say that CodexBar for Linux is required.

### P7 — Maintenance (optional)

A scheduled workflow that alerts when any of these change upstream:

- `Integrations/Linux/{main.cpp,DesktopController.cpp,Shared/Usage.js}`
- the README's "Adapter interface" section
- `ProviderIcon-*.svg`

andrew-verde's `tools/check-upstream-schema.py` shows the approach.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Upstream changes the socket path or the protocol | The `schemaVersion` gate and the `Incompatible` state; the P7 monitor; the protocol is documented for adapters |
| `xdg-run/…:create` behaves differently than expected | P0 item 3 tests it before any other work |
| Users install the desktop app but disable autostart | Autostart banner (§4.5) |
| The desktop app's tray icon duplicates the applet, and we can't read whether it's on | README and guidance text, and the one-time tray prompt (D8) |
| Users never install the desktop app | Guidance state (§4.4); the metainfo and README name the requirement |
| The desktop app exits on its own: it has no supervisor, and the README requires glibc 2.39+ and Qt 6.4+ | Same `Unavailable` path; the "isn't responding" wording when the socket is stale |
| Backend strings are English only | Accepted; flag it upstream if translations matter |
