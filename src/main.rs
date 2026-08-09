// SPDX-License-Identifier: MPL-2.0

mod alerts;
mod app;
mod codexbar;
mod config;
mod i18n;
mod popup;
mod privacy;
mod sessions;
mod settings;
mod spend;

#[derive(Debug, Default, Clone)]
pub struct Flags {
    open_popup: bool,
}

fn main() -> cosmic::iced::Result {
    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    // Starts the applet and honors the command exposed to COSMIC custom shortcuts.
    cosmic::applet::run::<app::AppModel>(Flags {
        open_popup: std::env::args().any(|argument| argument == "--open-popup"),
    })
}
