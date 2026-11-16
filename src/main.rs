// SPDX-License-Identifier: MPL-2.0

mod app;
// The panel and popup start using these in P2 (docs/implementation-plan.md).
#[allow(dead_code)]
mod backend;
mod i18n;
#[allow(dead_code)]
mod snapshot;

fn main() -> cosmic::iced::Result {
    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    // Starts the applet's event loop with `()` as the application's flags.
    cosmic::applet::run::<app::AppModel>(())
}
