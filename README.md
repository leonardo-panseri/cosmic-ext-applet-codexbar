# CodexBar applet for the COSMIC™ desktop

A third-party panel applet for the COSMIC™ desktop that shows AI usage from [CodexBar for Linux][codexbar-linux]. The desktop app
does the provider polling, settings, notifications and spending scans; this applet displays its
data and opens its windows. See [the implementation plan](docs/implementation-plan.md) for the design.
The earlier prototype, which called the `codexbar` CLI directly, is kept at the `prototype-cli` tag.

## Requirements

**CodexBar for Linux must be installed and running.** The applet reads its data over CodexBar's local
socket and shows nothing else without it. Install it by following the
[official installation guide][codexbar-linux-install].

Then, in CodexBar Settings:

- Keep **Start at login** on, so the applet has data after you log in. The applet shows a banner with a
  **Turn on** button when it's off.
- Turn off **Tray icon** (Settings → General, then **Save**), since this applet replaces it. The applet offers
  to do this once, or run:

  ```sh
  codexbar-linux --configure '{"showTray":false}'
  ```

## Install

The applet isn't published yet, so build the Flatpak locally. You need `flatpak`, [just][just], and:

```sh
flatpak install --user flathub org.flatpak.Builder \
    org.freedesktop.Sdk//25.08 org.freedesktop.Sdk.Extension.rust-stable//25.08 \
    com.system76.Cosmic.BaseApp//stable
```

Then, from this repository:

```sh
just flatpak-build     # builds into build-flatpak/repo (offline, from flatpak/cargo-sources.json)
just flatpak-install   # adds build-flatpak/repo as the `codexbar-applet-local` remote and installs from it
```

Open **Settings → Desktop → Panel → Configure panel applets**, press **Add applet** and choose **CodexBar**.

The panel icon in the popup's header opens the panel page, where you choose whether the panel button shows usage
bars, percentages or both, and which providers it shows. CodexBar only formats the bar for its first few providers
(**Providers in the bar** in CodexBar Settings, 2 by default), so set that to **All** to choose among every
provider.

## Development

A [justfile](./justfile) is included for the [casey/just][just] command runner.

- `just run` builds and runs the applet outside the panel
- `CODEXBAR_PREVIEW=tests/fixtures/snapshot-two-providers-spending.json cargo run` shows the panel button and the
  popup for a fixture in an ordinary window (debug builds only; `CODEXBAR_PREVIEW_PROVIDER=codex` picks the tab, `CODEXBAR_PREVIEW_PANEL_PAGE=1` opens
  the panel page)
- `cargo test` runs the unit tests; `cargo test -- --ignored live` also queries a running CodexBar (read-only)
- `just check --all-targets` runs clippy with pedantic lints; CI also runs `cargo fmt --check` and denies warnings
- `just flatpak-build` builds the working tree as a Flatpak; builder state stays in `.flatpak-builder/` and
  `build-flatpak/`, since `org.flatpak.Builder` has a private `/tmp`
- `just flatpak-sources` regenerates `flatpak/cargo-sources.json`; run it whenever `Cargo.lock` changes
- `just sync-icons <codexbar-checkout>` refreshes the provider logos and their licence notice

Developers should install [rustup][rustup] and configure their editor to use [rust-analyzer][rust-analyzer].

## Translators

[Fluent][fluent] is used for localization of the software. Fluent's translation files are found in the [i18n directory](./i18n). New translations may copy the [English (en) localization](./i18n/en) of the project, rename `en` to the desired [ISO 639-1 language code][iso-codes], and then translations can be provided for each [message identifier][fluent-guide]. If no translation is necessary, the message may be omitted.

## Licence

MPL-2.0. The provider logos in `resources/providers/` come from [CodexBar][codexbar] and are used under the MIT
licence; see [THIRD_PARTY_NOTICES.md](resources/THIRD_PARTY_NOTICES.md).

[fluent]: https://projectfluent.org/
[fluent-guide]: https://projectfluent.org/fluent/guide/hello.html
[iso-codes]: https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes
[just]: https://github.com/casey/just
[rustup]: https://rustup.rs/
[rust-analyzer]: https://rust-analyzer.github.io/
[codexbar]: https://github.com/steipete/CodexBar
[codexbar-linux]: https://github.com/steipete/CodexBar/blob/main/Integrations/Linux/README.md
[codexbar-linux-install]: https://github.com/steipete/CodexBar/blob/main/Integrations/Linux/README.md#install-release-archives
