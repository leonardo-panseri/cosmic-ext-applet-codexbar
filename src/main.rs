// SPDX-License-Identifier: MPL-2.0

mod app;
mod backend;
mod config;
mod guidance;
mod i18n;
mod icons;
mod popup;
#[cfg(debug_assertions)]
mod preview;
mod snapshot;

fn main() -> cosmic::iced::Result {
    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    #[cfg(debug_assertions)]
    if let Ok(path) = std::env::var("CODEXBAR_PREVIEW") {
        return preview::run(&path);
    }

    // Starts the applet's event loop with `()` as the application's flags.
    cosmic::applet::run::<app::AppModel>(())
}
