name := 'cosmic-ext-applet-codexbar'
appid := 'io.github.leonardo_panseri.cosmic-ext-applet-codexbar'

rootdir := ''
prefix := '/usr'

appdata := appid + '.metainfo.xml'
desktop := appid + '.desktop'

# Installation paths
base-dir := absolute_path(clean(rootdir / prefix))
cargo-target-dir := env('CARGO_TARGET_DIR', 'target')
appdata-dst := base-dir / 'share' / 'appdata' / appdata
bin-dst := base-dir / 'bin' / name
desktop-dst := base-dir / 'share' / 'applications' / desktop
icon-dst := base-dir / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / appid + '.svg'

# Default recipe which runs `just build-release`
default: build-release

# Runs `cargo clean`
clean:
    cargo clean

# Removes vendored dependencies
clean-vendor:
    rm -rf .cargo vendor vendor.tar

# `cargo clean` and removes vendored dependencies
clean-dist: clean clean-vendor

# Compiles with debug profile
build-debug *args:
    cargo build {{args}}

# Compiles with release profile
build-release *args: (build-debug '--release' args)

# Compiles release profile with vendored dependencies
build-vendored *args: vendor-extract (build-release '--frozen --offline' args)

# Runs a clippy check
check *args:
    cargo clippy --all-features {{args}} -- -W clippy::pedantic

# Runs a clippy check with JSON message format
check-json: (check '--message-format=json')

# Run the application for testing purposes
run *args:
    env RUST_BACKTRACE=full cargo run --release {{args}}

# Installs files
install:
    install -Dm0755 {{ cargo-target-dir / 'release' / name }} {{bin-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.desktop' }} {{desktop-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.metainfo.xml' }} {{appdata-dst}}
    install -Dm0644 resources/icon.svg {{icon-dst}}

# Uninstalls installed files
uninstall:
    rm {{bin-dst}} {{desktop-dst}} {{icon-dst}}

# Regenerates resources/screenshots/ from the test fixtures (needs a Wayland session)
screenshots *names:
    python3 tools/screenshots.py {{names}}

# Copies the provider logos and upstream's licence from a CodexBar checkout (plan D6)
sync-icons upstream:
    #!/usr/bin/env bash
    set -euo pipefail
    upstream='{{ join(invocation_directory(), upstream) }}'
    icons="$upstream/Sources/CodexBar/Resources"
    commit=$(git -C "$upstream" rev-parse --short=9 HEAD)
    mkdir -p resources/providers
    rm -f resources/providers/ProviderIcon-*.svg
    cp "$icons"/ProviderIcon-*.svg resources/providers/
    {
        echo '# Third-party notices'
        echo
        echo 'The provider logos in `resources/providers/` are copied from'
        echo "[steipete/CodexBar](https://github.com/steipete/CodexBar) \`$commit\`,"
        echo '`Sources/CodexBar/Resources/ProviderIcon-*.svg`, by `just sync-icons`.'
        echo 'They are used under the MIT licence:'
        echo
        echo '```text'
        cat "$upstream/LICENSE"
        echo '```'
    } > resources/THIRD_PARTY_NOTICES.md
    echo "Copied $(ls resources/providers | wc -l) logos from CodexBar $commit"

# Flatpak: the manifest builds the working tree, offline, from flatpak/cargo-sources.json.
# org.flatpak.Builder has a private /tmp, so its state and output stay under the repo.
flatpak-manifest := 'flatpak' / appid + '.json'
flatpak-state := '.flatpak-builder'
flatpak-build-dir := 'build-flatpak'
flatpak-repo := absolute_path(flatpak-build-dir / 'repo')
flatpak-remote := 'codexbar-applet-local'

# flatpak-cargo-generator, pinned; its Python venv lives in FLATPAK_TOOLS_DIR
cargo-generator-commit := '9d86d336a39b9af38b86f114b6ce5820fe260f91'
flatpak-tools := env('FLATPAK_TOOLS_DIR', flatpak-build-dir / 'tools')

# Builds the Flatpak into build-flatpak/repo (needs org.flatpak.Builder and the runtimes; see README).
# flatpak-builder runs `flatpak` on the host with its own environment, and the sandbox's
# XDG_DATA_HOME would hide runtimes installed with --user, so it is unset first.
flatpak-build *args:
    flatpak run --command=sh org.flatpak.Builder -c 'unset XDG_DATA_HOME; exec flatpak-builder "$@"' flatpak-builder \
        --force-clean \
        --state-dir={{flatpak-state}} \
        --repo={{flatpak-repo}} \
        {{args}} \
        {{flatpak-build-dir / 'app'}} {{flatpak-manifest}}

# Builds the Flatpak from the working tree, then installs or updates it for the current user.
# --or-update deploys next to the running applet; --reinstall uninstalls first, and the panel
# respawning the applet in between can fail it with "Directory not empty".
flatpak-install: flatpak-build
    flatpak remote-add --user --if-not-exists --no-gpg-verify {{flatpak-remote}} {{flatpak-repo}}
    flatpak install --user --or-update --noninteractive {{flatpak-remote}} {{appid}}

# Regenerates flatpak/cargo-sources.json from Cargo.lock; run after changing dependencies
flatpak-sources:
    #!/usr/bin/env bash
    set -euo pipefail
    tools='{{flatpak-tools}}'
    script="$tools/flatpak-cargo-generator.py"
    if [ ! -x "$tools/venv/bin/python" ]; then
        python3 -m venv "$tools/venv"
        "$tools/venv/bin/pip" install --quiet 'aiohttp>=3.9.5,<4' 'tomlkit>=0.13.3,<1'
    fi
    if [ ! -f "$script" ]; then
        curl -fsSL -o "$script" \
            'https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/{{cargo-generator-commit}}/cargo/flatpak-cargo-generator.py'
    fi
    "$tools/venv/bin/python" -I "$script" Cargo.lock -o flatpak/cargo-sources.json

# Writes the pop-os/cosmic-flatpak copy of the manifest for a pushed tag into build-flatpak/cosmic-flatpak/:
# the dir source becomes the tag and its commit, and cargo-sources.json is copied unchanged.
# COSMIC_FLATPAK_URL overrides the repository URL, for a local test build before the tag is pushed.
cosmic-flatpak tag:
    #!/usr/bin/env bash
    set -euo pipefail
    commit=$(git rev-parse --verify '{{tag}}^{commit}')
    url="${COSMIC_FLATPAK_URL:-https://github.com/leonardo-panseri/cosmic-ext-applet-codexbar.git}"
    out='{{flatpak-build-dir}}/cosmic-flatpak/app/{{appid}}'
    rm -rf "$out"
    mkdir -p "$out"
    jq --arg url "$url" --arg tag '{{tag}}' --arg commit "$commit" \
        '.modules[0].sources[0] = {type: "git", url: $url, tag: $tag, commit: $commit}' \
        {{flatpak-manifest}} > "$out/{{appid}}.json"
    cp flatpak/cargo-sources.json "$out/"
    echo "Wrote $out for {{tag}} ($commit)"

# Vendor dependencies locally
vendor:
    mkdir -p .cargo
    cargo vendor --sync Cargo.toml | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    echo >> .cargo/config.toml
    rm -rf .cargo vendor

# Extracts vendored dependencies
vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar

# Bump cargo version, create git commit, and create tag
tag version:
    find -type f -name Cargo.toml -exec sed -i '0,/^version/s/^version.*/version = "{{version}}"/' '{}' \; -exec git add '{}' \;
    cargo check
    cargo clean
    git add Cargo.lock
    git commit -m 'release: {{version}}'
    git tag -a {{version}} -m ''

