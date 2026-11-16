// SPDX-License-Identifier: MPL-2.0

//! The applet's own icon and upstream's provider logos, embedded in the binary because the
//! sandbox can't read the desktop app's install (plan D6).
//!
//! Every icon is drawn symbolic, so it takes the panel's text colour as Omarchy's widget does:
//! black logos tint cleanly, and multi-colour ones become flat silhouettes.

use cosmic::widget::icon;
use rust_embed::RustEmbed;

/// `ProviderIcon-{provider}.svg` from upstream, refreshed by `just sync-icons`.
#[derive(RustEmbed)]
#[folder = "resources/providers/"]
struct Providers;

/// The applet's icon, the one installed under the app ID.
pub fn app() -> icon::Handle {
    symbolic(icon::from_svg_bytes(
        include_bytes!("../resources/icon.svg").as_slice(),
    ))
}

/// The provider's logo, or `None` when upstream has none, in which case the caller shows the
/// bar entry's tag instead.
pub fn provider(provider: &str) -> Option<icon::Handle> {
    let file = Providers::get(&file_name(provider)?)?;
    Some(symbolic(icon::from_svg_bytes(file.data)))
}

/// Provider ids are lowercase words such as `claude` or `opencodego`. Anything else can't name a
/// vendored file, and is rejected before it reaches a path.
fn file_name(provider: &str) -> Option<String> {
    let valid = !provider.is_empty()
        && provider
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    valid.then(|| format!("ProviderIcon-{provider}.svg"))
}

fn symbolic(mut handle: icon::Handle) -> icon::Handle {
    // A public field, not a builder method (ADR 0001, P0 item 4).
    handle.symbolic = true;
    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_vendored_file_is_a_provider_logo() {
        let names: Vec<_> = Providers::iter().collect();
        assert!(names.len() >= 100, "only {} logos", names.len());
        for name in names {
            let id = name
                .strip_prefix("ProviderIcon-")
                .and_then(|n| n.strip_suffix(".svg"))
                .unwrap_or_else(|| panic!("unexpected file {name}"));
            assert_eq!(file_name(id).as_deref(), Some(&*name));
            let svg = Providers::get(&name).unwrap().data;
            assert!(svg.windows(4).any(|w| w == b"<svg"), "{name} isn't an SVG");
        }
    }

    #[test]
    fn fixture_providers_have_logos() {
        for id in ["claude", "codex", "gemini"] {
            assert!(provider(id).is_some_and(|h| h.symbolic), "{id}");
        }
    }

    #[test]
    fn unknown_or_unsafe_providers_have_no_logo() {
        assert!(provider("not-a-provider").is_none());
        assert!(provider("").is_none());
        assert!(provider("../icon").is_none());
        assert!(provider("claude/../codex").is_none());
        assert!(provider("claude.svg").is_none());
    }

    #[test]
    fn app_icon_is_symbolic() {
        assert!(app().symbolic);
    }
}
