// SPDX-License-Identifier: MPL-2.0

//! What the popup says when the user has something to do: the guidance in the `Connecting`,
//! `Unavailable` and `Incompatible` states (plan D7, §4.4), and the two banners of the `Connected`
//! popup, for autostart (§4.5) and the tray icon (D8).
//!
//! Which banner shows, and what it says, is decided by [`Autostart`] and [`TrayPrompt`], kept apart
//! from the GUI so they can be tested. Neither changes anything in the desktop app on its own: each
//! request follows a click.

use cosmic::applet;
use cosmic::prelude::*;
use cosmic::widget;

use crate::app::{Message, Unavailable};
use crate::backend::BackendError;
use crate::fl;

/// The official installation guide for the desktop app.
pub const INSTALL_GUIDE: &str = "https://github.com/steipete/CodexBar/blob/main/Integrations/Linux/README.md#install-release-archives";
/// This applet's repository, for updates when the desktop app is newer than it.
pub const APPLET_REPO: &str = "https://github.com/leonardo-panseri/cosmic-ext-applet-codexbar";

/// Whether the desktop app starts at login, and the **Turn on** button's progress.
#[derive(Debug, Default)]
pub struct Autostart {
    /// As the desktop app last said. `None` until asked, or when it couldn't say, for example an
    /// older desktop app without the command: then there's no banner.
    enabled: Option<bool>,
    /// **Turn on** was clicked, and the status check after it hasn't returned yet.
    enabling: bool,
    error: Option<String>,
}

impl Autostart {
    /// The popup opened. The caller checks the status again; an old error goes.
    pub fn popup_opened(&mut self) {
        self.error = None;
    }

    /// The reply to `autostart status`.
    pub fn checked(&mut self, result: Result<bool, BackendError>) {
        self.enabled = result.ok();
        self.enabling = false;
    }

    /// **Turn on** was clicked. `true` means "send `autostart enable` now".
    pub fn turn_on(&mut self) -> bool {
        if self.enabling {
            return false;
        }
        self.enabling = true;
        self.error = None;
        true
    }

    /// The reply to `autostart enable`. The caller checks the status after it either way, and
    /// that check ends `enabling`.
    pub fn enable_replied(&mut self, result: &Result<bool, BackendError>) {
        if let Err(why) = result {
            self.error = Some(failure(why));
        }
    }

    /// The banner, while the desktop app says autostart is off.
    pub fn banner(&self) -> Option<Banner<'_>> {
        (self.enabled == Some(false)).then_some(Banner {
            busy: self.enabling,
            error: self.error.as_deref(),
        })
    }
}

/// The one-time prompt to hide the desktop app's tray icon (plan D8).
#[derive(Debug, Default)]
pub struct TrayPrompt {
    /// Answered, now or in an earlier session.
    dismissed: bool,
    /// **Hide tray icon** was clicked and the desktop app hasn't replied yet.
    hiding: bool,
    error: Option<String>,
}

impl TrayPrompt {
    pub fn new(dismissed: bool) -> Self {
        Self {
            dismissed,
            ..Self::default()
        }
    }

    /// The popup opened: an old error goes.
    pub fn popup_opened(&mut self) {
        self.error = None;
    }

    /// The stored flag changed, for example when the applet's instance on another panel was
    /// answered.
    pub fn stored(&mut self, dismissed: bool) {
        self.dismissed = dismissed;
    }

    /// **Hide tray icon** was clicked. `true` means "send `configure` now".
    pub fn hide(&mut self) -> bool {
        if self.hiding || self.dismissed {
            return false;
        }
        self.hiding = true;
        self.error = None;
        true
    }

    /// The reply to `configure`. `true` means "store the dismissal". When the desktop app refused,
    /// the prompt stays with the reason, so the user can try again or keep the icon.
    pub fn hide_replied(&mut self, result: &Result<(), BackendError>) -> bool {
        self.hiding = false;
        match result {
            Ok(()) => self.dismiss(),
            Err(why) => {
                self.error = Some(failure(why));
                false
            }
        }
    }

    /// **Keep it** was clicked. `true` means "store the dismissal".
    pub fn keep(&mut self) -> bool {
        self.dismiss()
    }

    fn dismiss(&mut self) -> bool {
        self.error = None;
        !std::mem::replace(&mut self.dismissed, true)
    }

    /// The prompt, until it's answered.
    pub fn banner(&self) -> Option<Banner<'_>> {
        (!self.dismissed).then_some(Banner {
            busy: self.hiding,
            error: self.error.as_deref(),
        })
    }
}

/// A banner in the `Connected` popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Banner<'a> {
    /// Its request is running: the button that sent it is disabled.
    pub busy: bool,
    /// Why the last request failed.
    pub error: Option<&'a str>,
}

/// Why a request from a banner failed: the desktop app's own reason when it gave one.
pub fn failure(why: &BackendError) -> String {
    match why {
        BackendError::Rejected(reason) if !reason.is_empty() => reason.clone(),
        BackendError::Rejected(_) => fl!("request-refused"),
        _ => fl!("request-failed"),
    }
}

/// The heading and paragraphs of the `Unavailable` popup. A socket that refuses connections
/// means the desktop app is installed, so the text suggests restarting it (plan D4).
pub fn unavailable_text(why: Unavailable) -> (String, [String; 2]) {
    let (heading, body) = match why {
        Unavailable::NotRunning => (fl!("not-running"), fl!("not-running-body")),
        Unavailable::NotResponding => (fl!("not-responding"), fl!("not-responding-body")),
    };
    (heading, [body, fl!("desktop-app-settings")])
}

pub fn connecting<'a>() -> Element<'a, Message> {
    page(fl!("app-title"), [fl!("connecting")], Vec::new())
}

pub fn unavailable<'a>(why: Unavailable) -> Element<'a, Message> {
    let (heading, paragraphs) = unavailable_text(why);
    page(
        heading,
        paragraphs,
        vec![
            widget::button::suggested(fl!("open-install-guide"))
                .on_press(Message::OpenUrl(INSTALL_GUIDE))
                .into(),
            widget::button::standard(fl!("check-again"))
                .on_press(Message::CheckAgain)
                .into(),
        ],
    )
}

pub fn incompatible<'a>() -> Element<'a, Message> {
    page(
        fl!("app-title"),
        [fl!("incompatible")],
        vec![
            widget::button::standard(fl!("open-install-guide"))
                .on_press(Message::OpenUrl(INSTALL_GUIDE))
                .into(),
            widget::button::standard(fl!("open-applet-page"))
                .on_press(Message::OpenUrl(APPLET_REPO))
                .into(),
        ],
    )
}

/// The autostart banner (plan §4.5).
pub fn autostart_banner(banner: Banner<'_>) -> Element<'_, Message> {
    banner_view(
        fl!("autostart-off"),
        banner.error,
        vec![
            widget::button::suggested(fl!("turn-on"))
                .on_press_maybe((!banner.busy).then_some(Message::TurnOnAutostart))
                .into(),
        ],
    )
}

/// The tray prompt (plan D8).
pub fn tray_banner(banner: Banner<'_>) -> Element<'_, Message> {
    banner_view(
        fl!("tray-prompt"),
        banner.error,
        vec![
            widget::button::standard(fl!("keep-tray"))
                .on_press(Message::KeepTray)
                .into(),
            widget::button::suggested(fl!("hide-tray"))
                .on_press_maybe((!banner.busy).then_some(Message::HideTray))
                .into(),
        ],
    )
}

/// A whole popup: heading, paragraphs and a row of buttons.
fn page(
    heading: String,
    paragraphs: impl IntoIterator<Item = String>,
    buttons: Vec<Element<'_, Message>>,
) -> Element<'_, Message> {
    let column = paragraphs.into_iter().fold(
        widget::Column::new()
            .spacing(8)
            .push(widget::text::heading(heading)),
        |column, paragraph| column.push(widget::text::body(paragraph)),
    );
    let column = if buttons.is_empty() {
        column
    } else {
        column.push(buttons_row(buttons))
    };
    widget::container(applet::padded_control(column))
        .padding([8, 0])
        .into()
}

fn banner_view<'a>(
    text: String,
    error: Option<&'a str>,
    buttons: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut column = widget::Column::new()
        .spacing(8)
        .push(widget::text::body(text));
    if let Some(error) = error {
        column = column.push(
            widget::text::caption(error).class(cosmic::theme::Text::Custom(|theme| {
                cosmic::iced::widget::text::Style {
                    color: Some(theme.cosmic().destructive_text_color().into()),
                    ..cosmic::iced::widget::text::Catalog::style(
                        theme,
                        &cosmic::theme::Text::Default,
                    )
                }
            })),
        );
    }
    widget::container(column.push(buttons_row(buttons)))
        .padding(12)
        .width(cosmic::iced::Length::Fill)
        .class(cosmic::theme::Container::Card)
        .into()
}

/// Buttons aligned to the end, as in COSMIC dialogs.
fn buttons_row(buttons: Vec<Element<'_, Message>>) -> Element<'_, Message> {
    widget::container(widget::Row::with_children(buttons).spacing(8))
        .align_right(cosmic::iced::Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejected(reason: &str) -> BackendError {
        BackendError::Rejected(reason.into())
    }

    #[test]
    fn unavailable_wording_depends_on_why() {
        let (heading, [body, settings]) = unavailable_text(Unavailable::NotRunning);
        assert_eq!(heading, "CodexBar for Linux isn't running");
        assert!(body.contains("official installation guide"), "{body}");
        assert!(settings.contains("Start at login"), "{settings}");
        assert!(settings.contains("Tray icon"), "{settings}");

        let (heading, [body, settings_again]) = unavailable_text(Unavailable::NotResponding);
        assert_eq!(heading, "CodexBar for Linux isn't responding");
        assert!(body.contains("Restart it"), "{body}");
        assert_eq!(settings_again, settings);
    }

    #[test]
    fn failures_show_the_desktop_apps_reason() {
        assert_eq!(
            failure(&rejected("linux.json is invalid")),
            "linux.json is invalid"
        );
        assert_eq!(failure(&rejected("")), "CodexBar refused the change.");
        assert_eq!(
            failure(&BackendError::NotRunning),
            "CodexBar didn't make the change. Try again."
        );
    }

    #[test]
    fn autostart_banner_only_while_off() {
        let mut autostart = Autostart::default();
        assert_eq!(autostart.banner(), None);
        autostart.checked(Ok(true));
        assert_eq!(autostart.banner(), None);
        autostart.checked(Ok(false));
        assert_eq!(
            autostart.banner(),
            Some(Banner {
                busy: false,
                error: None
            })
        );
        // An older desktop app without the command, or one that went away.
        autostart.checked(Err(rejected("Unknown command")));
        assert_eq!(autostart.banner(), None);
    }

    #[test]
    fn turn_on_then_recheck_removes_the_banner() {
        let mut autostart = Autostart::default();
        autostart.checked(Ok(false));
        assert!(autostart.turn_on());
        assert!(!autostart.turn_on(), "one request at a time");
        assert_eq!(autostart.banner().map(|b| b.busy), Some(true));
        autostart.enable_replied(&Ok(true));
        assert_eq!(autostart.banner().map(|b| b.busy), Some(true));
        autostart.checked(Ok(true));
        assert_eq!(autostart.banner(), None);
    }

    #[test]
    fn failed_turn_on_shows_why_until_the_next_try() {
        let mut autostart = Autostart::default();
        autostart.checked(Ok(false));
        assert!(autostart.turn_on());
        autostart.enable_replied(&Err(rejected("can't write the autostart entry")));
        autostart.checked(Ok(false));
        assert_eq!(
            autostart.banner(),
            Some(Banner {
                busy: false,
                error: Some("can't write the autostart entry")
            })
        );
        assert!(autostart.turn_on());
        assert_eq!(
            autostart.banner(),
            Some(Banner {
                busy: true,
                error: None
            })
        );
        autostart.enable_replied(&Err(rejected("again")));
        autostart.checked(Ok(false));
        autostart.popup_opened();
        assert_eq!(autostart.banner().and_then(|b| b.error), None);
    }

    #[test]
    fn tray_prompt_shows_until_answered() {
        assert_eq!(TrayPrompt::new(true).banner(), None);
        let mut prompt = TrayPrompt::new(false);
        assert_eq!(
            prompt.banner(),
            Some(Banner {
                busy: false,
                error: None
            })
        );
        assert!(prompt.keep(), "store the dismissal");
        assert_eq!(prompt.banner(), None);
        assert!(!prompt.keep(), "already stored");
        assert!(!prompt.hide(), "nothing to ask once answered");
    }

    #[test]
    fn hiding_the_tray_dismisses_the_prompt() {
        let mut prompt = TrayPrompt::new(false);
        assert!(prompt.hide());
        assert!(!prompt.hide(), "one request at a time");
        assert_eq!(prompt.banner().map(|b| b.busy), Some(true));
        assert!(prompt.hide_replied(&Ok(())));
        assert_eq!(prompt.banner(), None);
    }

    #[test]
    fn refused_hide_keeps_the_prompt_with_the_reason() {
        let mut prompt = TrayPrompt::new(false);
        assert!(prompt.hide());
        assert!(!prompt.hide_replied(&Err(rejected("linux.json is invalid"))));
        assert_eq!(
            prompt.banner(),
            Some(Banner {
                busy: false,
                error: Some("linux.json is invalid")
            })
        );
        prompt.popup_opened();
        assert_eq!(prompt.banner().and_then(|b| b.error), None);
        // Giving up on hiding it is still an answer.
        assert!(prompt.keep());
        assert_eq!(prompt.banner(), None);
    }

    #[test]
    fn tray_prompt_follows_the_stored_flag() {
        let mut prompt = TrayPrompt::new(false);
        prompt.stored(true);
        assert_eq!(prompt.banner(), None);
        prompt.stored(false);
        assert!(prompt.banner().is_some());
    }
}
