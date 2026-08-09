// SPDX-License-Identifier: MPL-2.0

//! Provides localization support for this crate.

use std::sync::LazyLock;

use i18n_embed::{
    DefaultLocalizer, DesktopLanguageRequester, LanguageLoader, Localizer,
    fluent::{FluentLanguageLoader, fluent_language_loader},
    unic_langid::LanguageIdentifier,
};
use rust_embed::RustEmbed;

/// Applies the requested language(s) to requested translations from the `fl!()` macro.
pub fn init(requested_languages: &[LanguageIdentifier]) {
    if let Err(why) = localizer().select(requested_languages) {
        eprintln!("error while loading fluent localizations: {why}");
    }
}

pub fn select_override(language_override: &str) {
    let system = DesktopLanguageRequester::requested_languages();
    init(&language_preferences(language_override, &system));
}

fn language_preferences(
    language_override: &str,
    system: &[LanguageIdentifier],
) -> Vec<LanguageIdentifier> {
    language_override
        .trim()
        .parse::<LanguageIdentifier>()
        .ok()
        .map(|language| vec![language])
        .unwrap_or_else(|| system.to_vec())
}

// Get the `Localizer` to be used for localizing this library.
#[must_use]
pub fn localizer() -> Box<dyn Localizer> {
    Box::from(DefaultLocalizer::new(&*LANGUAGE_LOADER, &Localizations))
}

#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

pub static LANGUAGE_LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader: FluentLanguageLoader = fluent_language_loader!();

    loader
        .load_fallback_language(&Localizations)
        .expect("Error while loading fallback language");

    loader
});

/// Request a localized string by ID from the i18n/ directory.
#[macro_export]
macro_rules! fl {
    ($message_id:literal) => {{
        i18n_embed_fl::fl!($crate::i18n::LANGUAGE_LOADER, $message_id)
    }};

    ($message_id:literal, $($args:expr),*) => {{
        i18n_embed_fl::fl!($crate::i18n::LANGUAGE_LOADER, $message_id, $($args), *)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_or_invalid_override_uses_system_and_valid_override_wins() {
        let system = vec!["it-IT".parse().unwrap(), "en-US".parse().unwrap()];

        assert_eq!(language_preferences("", &system), system);
        assert_eq!(language_preferences("not a locale!", &system), system);
        assert_eq!(
            language_preferences(" fr-FR ", &system),
            ["fr-FR".parse::<LanguageIdentifier>().unwrap()]
        );
    }
}
