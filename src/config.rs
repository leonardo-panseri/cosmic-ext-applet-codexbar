// SPDX-License-Identifier: MPL-2.0

//! The applet's only stored state (plan D3): whether the tray prompt (D8) has been answered, and
//! what the panel button shows, set on the popup's panel page. Every other display preference
//! lives in the desktop app.

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub struct Config {
    /// The user clicked **Keep it**, or **Hide tray icon** and the desktop app hid it.
    pub tray_prompt_dismissed: bool,
    /// Providers turned off on the popup's panel page. A list of the hidden ones, so providers
    /// turned on later in the desktop app show by default.
    pub hidden_providers: Vec<String>,
    pub panel_style: PanelStyle,
}

/// What the panel button shows next to each provider's logo. At least one of the two, so the
/// choice is one of three.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelStyle {
    /// Small bars for the provider's windows, then its percentage.
    #[default]
    Both,
    Bars,
    Percent,
}

impl PanelStyle {
    pub const ALL: [Self; 3] = [Self::Bars, Self::Percent, Self::Both];

    pub fn bars(self) -> bool {
        matches!(self, Self::Both | Self::Bars)
    }

    pub fn percent(self) -> bool {
        matches!(self, Self::Both | Self::Percent)
    }
}

/// Opens `~/.config/cosmic/{app_id}/v1` and reads it. Without it, nothing is remembered: the
/// tray prompt then shows again after a restart.
pub fn load(app_id: &str) -> (Option<cosmic_config::Config>, Config) {
    match cosmic_config::Config::new(app_id, Config::VERSION) {
        Ok(handler) => {
            let config = read(&handler);
            (Some(handler), config)
        }
        Err(why) => {
            eprintln!("can't open the applet's config: {why}");
            (None, Config::default())
        }
    }
}

/// The stored values, with defaults for anything missing or unreadable.
pub fn read(handler: &cosmic_config::Config) -> Config {
    Config::get_entry(handler).unwrap_or_else(|(errors, config)| {
        for why in errors {
            eprintln!("can't read the applet's config: {why}");
        }
        config
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismissal_is_stored_under_the_app_id() {
        let dir = tempfile::tempdir().unwrap();
        let app_id = "io.github.example.test";
        let handler =
            cosmic_config::Config::with_custom_path(app_id, Config::VERSION, dir.path().into())
                .unwrap();
        let mut config = read(&handler);
        assert_eq!(config, Config::default());

        assert!(config.set_tray_prompt_dismissed(&handler, true).unwrap());
        assert!(
            dir.path()
                .join("cosmic")
                .join(app_id)
                .join("v1")
                .join("tray_prompt_dismissed")
                .is_file()
        );

        assert!(config.set_panel_style(&handler, PanelStyle::Bars).unwrap());
        assert!(
            config
                .set_hidden_providers(&handler, vec!["codex".into()])
                .unwrap()
        );

        let reopened =
            cosmic_config::Config::with_custom_path(app_id, Config::VERSION, dir.path().into())
                .unwrap();
        let stored = read(&reopened);
        assert!(stored.tray_prompt_dismissed);
        assert_eq!(stored.panel_style, PanelStyle::Bars);
        assert_eq!(stored.hidden_providers, ["codex"]);
    }
}
