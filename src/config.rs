// SPDX-License-Identifier: MPL-2.0

//! The applet's only stored state (plan D3): whether the tray prompt (D8) has been answered.
//! There is no settings UI; every display preference lives in the desktop app.

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};

#[derive(Debug, Default, Clone, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub struct Config {
    /// The user clicked **Keep it**, or **Hide tray icon** and the desktop app hid it.
    pub tray_prompt_dismissed: bool,
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

        let reopened =
            cosmic_config::Config::with_custom_path(app_id, Config::VERSION, dir.path().into())
                .unwrap();
        assert!(read(&reopened).tray_prompt_dismissed);
    }
}
