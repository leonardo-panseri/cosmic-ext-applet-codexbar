# CodexBar applet for the COSMIC™ desktop

A third-party panel applet for the COSMIC™ desktop that shows AI usage from [CodexBar for Linux][codexbar-linux]. The desktop app
does the provider polling, settings, notifications and spending scans; this applet displays its
data and opens its windows. It is being rebuilt; see [the implementation plan](docs/implementation-plan.md).
The earlier prototype, which called the `codexbar` CLI directly, is kept at the `prototype-cli` tag.

## Requirements

Install CodexBar for Linux by following the [official installation guide][codexbar-linux-install],
and keep **Start at login** enabled in its Settings so the applet has data after you log in.

This applet replaces the desktop app's own tray icon. Turn off **Tray icon** in CodexBar Settings → General and press
**Save**, or run:

```sh
codexbar-linux --configure '{"showTray":false}'
```

## Building

A [justfile](./justfile) is included by default for the [casey/just][just] command runner.

- `just` builds the application with the default `just build-release` recipe
- `just run` builds and runs the application
- `just install` installs the project into the system
- `just check` runs clippy on the project to check for linter warnings
- `just check-json` can be used by IDEs that support LSP

## Translators

[Fluent][fluent] is used for localization of the software. Fluent's translation files are found in the [i18n directory](./i18n). New translations may copy the [English (en) localization](./i18n/en) of the project, rename `en` to the desired [ISO 639-1 language code][iso-codes], and then translations can be provided for each [message identifier][fluent-guide]. If no translation is necessary, the message may be omitted.

## Developers

Developers should install [rustup][rustup] and configure their editor to use [rust-analyzer][rust-analyzer].

[fluent]: https://projectfluent.org/
[fluent-guide]: https://projectfluent.org/fluent/guide/hello.html
[iso-codes]: https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes
[just]: https://github.com/casey/just
[rustup]: https://rustup.rs/
[rust-analyzer]: https://rust-analyzer.github.io/
[sccache]: https://github.com/mozilla/sccache
[codexbar-linux]: https://github.com/steipete/CodexBar/blob/main/Integrations/Linux/README.md
[codexbar-linux-install]: https://github.com/steipete/CodexBar/blob/main/Integrations/Linux/README.md#install-release-archives
