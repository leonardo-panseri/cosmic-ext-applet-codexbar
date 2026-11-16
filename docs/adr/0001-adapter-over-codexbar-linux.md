---
status: accepted
---

# The applet is a display-only adapter that talks to `codexbar-linux` over its socket

The applet gets every number it shows from the upstream desktop app, `codexbar-linux`. It never runs the
`codexbar` CLI or reads provider credentials. It connects directly to the desktop app's local socket,
`$XDG_RUNTIME_DIR/codexbar-linux/desktop.sock`, and does not go through `codexbar-linux --snapshot`.

Polling, notifications, spending scans and display preferences live in the desktop app once, and the applet
stays small. Using the socket directly means the Flatpak needs only `--filesystem=xdg-run/codexbar-linux:create`.
Running the client binary would need a host-escape permission, and would cost a Qt process every 5 s.

## Considered options

- **Call the `codexbar` CLI (the prototype, tag `prototype-cli`).** Rejected: it duplicates the desktop app's
  provider logic, polling and notifications, and it needs credentials and network access in the sandbox.
- **Run `codexbar-linux --snapshot` through `flatpak-spawn --host`.** Rejected: it needs
  `--talk-name=org.freedesktop.Flatpak`, and it gives no protection from protocol changes because it copies the
  socket reply unchanged.

## Consequences

- The applet is useless without the desktop app installed and running on the host. It shows install guidance
  when it can't connect, and it can't go on Flathub ("relies on host components"), so it ships through
  pop-os/cosmic-flatpak.
- The applet depends on snapshot `schemaVersion` 1. Any other version shows an "Incompatible" state.

## P0 spike results (2026-11-16)

Tested on CachyOS with cosmic-panel, Flatpak 1.18.4, and `codexbar-linux` **0.84.0**. The plan pins upstream
`1f6da9bec` (0.84.1), but `Integrations/Linux` hasn't changed since `ed68b1013`, so the protocol is the same.
The spike code is on the throwaway branch `spike/p0-sandbox`.

1. **Install the desktop app.** Pass. The Qt packages and `codexbar-linux` were already installed and running.
2. **Fixtures.** Pass. Ten fixtures are in `tests/fixtures/`, each case with and without `"spending":true`:
   `two-providers`, `one-provider`, `provider-error` (an unconfigured `gemini`), `quota-used` and `bar-detail`.
   They were captured by switching settings with `configure` and then restoring them exactly. Scrubbed: `pid`,
   the org plan name, the spending amounts, and the scoped-cap key hash.
   - The snapshot has fields the plan doesn't model: `pid`, `costProviders` and `windows[].resetsAt`.
   - `summary` already ends in `+N` when some entries don't fit the bar (`"CL 90%  ·  CX 100%  +1"`), but
     `barEntries` doesn't, so the panel adds `+N` itself as §4.2 says.
   - With `showBarDetail` on, a bar entry's `text` reads like `5H 90% · 7D 84% · -30%`.
   - A desktop app that has just started returns `summary: ""` and no entries for about 2 s, so the "empty
     snapshot → app icon" path in §4.2 is real.
3. **Socket access from the sandbox.** Pass, so D2 stands. Tested with the P5 `finish-args`:
   - **Desktop app first:** a snapshot comes back in about 0.5 ms.
   - **Applet first:** Flatpak creates the folder with mode `0755`. The desktop app accepts it (same owner, not
     a symlink) and changes it to `0700`. The sandbox then connects with nothing restarted.
   - **Desktop app restart:** a clean `--quit` deletes the socket, so the sandbox sees `ENOENT` and reconnects
     once the new instance listens. After a `kill -9` the stale socket gives `ECONNREFUSED`, so D4's
     "isn't responding" wording can be detected. The next instance takes over the stale lock without trouble.
4. **Icon tint.** Pass. `icon::from_svg_bytes(..)` with `handle.symbolic = true` (`Handle` has a public field,
   not a builder method) draws the Claude and Codex logos in the panel's text colour, as checked on a
   screenshot. Tiny-skia replaces each pixel's colour and keeps its alpha. That gives the same result as
   Omarchy's `MultiEffect` chain: black logos tint correctly, and multi-colour or gradient logos become flat
   silhouettes.
5. **App ID.** Pass. `flatpak build-init` accepts `io.github.leonardo_panseri.cosmic-ext-applet-codexbar`, and
   rejects the old ID with "Only last name segment can contain -".
6. **Panel registration.** Pass, after two fixes:
   - **`Exec` must not contain `%F`.** cosmic-panel inserts `--socket=inherit-wayland-socket` and its `--env`
     flags at position `len-2` of the exported `flatpak run` command. With `%F`, the export ends in
     `--file-forwarding <id> @@ %F @@`, the flag lands inside the file-forwarding block, and the applet
     crash-loops ("Failed to open '/home/…/--socket=inherit-wayland-socket'"). Without it, cosmic-panel
     launches the Flatpak applet and it polls the socket every 5 s.
   - **The panel view must be wrapped in `core.applet.autosize_window(..)`.** Otherwise the panel clips the
     button to icon size, and only the first logo shows.
   - Also for P5: `appstreamcli compose` fails without a `<description>`, and the metainfo's remote icon URL
     (`….git/raw/main/resources/icons/…`) points nowhere.
7. **Software renderer.** The template already renders with tiny-skia. libcosmic's `wgpu` feature is off,
   and `iced_wgpu` is compiled only because the `wayland` feature pulls it in. Measured over a 12 s idle
   standalone run:

   | Build | RSS | PSS | Private dirty | Binary |
   |---|---|---|---|---|
   | Template features (tiny-skia) | 35 MB | 27 MB | 23 MB | 32 MB |
   | Template features + `wgpu` | 71 MB | 36 MB | 33 MB | 40 MB |
   | `default-features = false`, no `a11y`/`x11` | 32 MB | 24 MB | 20 MB | 27 MB |

   The tiny-skia Flatpak build in the real panel used 28 MB RSS and drew correctly. **Decision:** never enable
   `wgpu`. Set `default-features = false` and list the features explicitly, dropping `x11`, since the panel is
   Wayland-only. Whether to keep `a11y` is decided in P2: the 3 MB saving above covers `a11y` and `x11`
   together. The no-default build hasn't been checked in the real panel yet.
