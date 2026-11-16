// SPDX-License-Identifier: MPL-2.0

//! The popup in the `Connected` state (plan §4.3): a status line, one card per entry in a single
//! scrolling list, and the footer actions.
//!
//! What the popup says is decided by the pure functions here, so it can be tested against the
//! fixtures. Text from the backend (labels, reset text, pace, plan, status and errors) is shown as
//! it comes (§4.6). Number formats follow upstream's `Integrations/Linux/Shared/Usage.js` and the
//! desktop app's `QuickView.qml`.

use cosmic::applet;
use cosmic::iced::keyboard::{self, Key, key::Named};
use cosmic::iced::widget::progress_bar;
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget::{self, icon};

use crate::app::Message;
use crate::backend;
use crate::snapshot::{Entry, ExtraUsage, ResetCredits, Snapshot, Spending, Window};
use crate::{fl, icons};

/// The card list scrolls beyond this, so the popup stays on screen with many providers.
const LIST_MAX_HEIGHT: f32 = 520.0;
const LOGO_SIZE: u16 = 20;
const STATUS_DOT: f32 = 8.0;

pub fn view(snapshot: &Snapshot) -> Element<'_, Message> {
    let header = widget::Column::new()
        .spacing(2)
        .push(widget::text::heading(fl!("app-title")))
        .push(widget::text::caption(status_line(snapshot)));

    let cards = snapshot.entries.iter().fold(
        widget::Column::new().spacing(8).padding([0, 12]),
        |cards, entry| cards.push(card(entry, spending_for(snapshot, &entry.provider))),
    );

    let footer = [
        (fl!("refresh"), (!snapshot.busy).then_some(Message::Refresh)),
        (
            fl!("usage-and-spend"),
            Some(Message::Open(backend::Window::Usage)),
        ),
        (
            fl!("settings"),
            Some(Message::Open(backend::Window::Settings)),
        ),
    ]
    .into_iter()
    .fold(widget::Column::new(), |footer, (label, message)| {
        footer.push(applet::menu_button(widget::text::body(label)).on_press_maybe(message))
    });

    widget::Column::new()
        .spacing(8)
        .padding([8, 0])
        .push(applet::padded_control(header))
        .push(widget::container(widget::scrollable(cards)).max_height(LIST_MAX_HEIGHT))
        .push(applet::padded_control(
            widget::divider::horizontal::default(),
        ))
        .push(footer)
        .into()
}

fn card<'a>(entry: &'a Entry, spending: Option<&'a Spending>) -> Element<'a, Message> {
    let mut header = widget::Row::new().spacing(8).align_y(Alignment::Center);
    if let Some(logo) = icons::provider(&entry.provider) {
        header = header.push(icon::icon(logo).size(LOGO_SIZE));
    }
    header = header.push(widget::text::heading(display_name(&entry.provider)).width(Length::Fill));
    if !entry.plan.is_empty() {
        header = header.push(widget::text::caption(entry.plan.as_str()));
    }

    let mut card = widget::Column::new().spacing(8).push(header);
    if let Some((severity, status)) = status(entry) {
        card = card.push(
            widget::Row::new()
                .spacing(6)
                .align_y(Alignment::Center)
                .push(dot(severity))
                .push(widget::text::caption(status)),
        );
    }
    if !entry.error.is_empty() {
        card = card.push(widget::text::body(entry.error.as_str()));
    }
    for window in &entry.windows {
        card = card.push(window_view(window));
    }
    let extras = [
        entry.credits.map(credits_line),
        entry.extra_usage.as_ref().and_then(extra_usage_line),
        entry.reset_credits.as_ref().map(reset_credits_line),
        spending.map(spending_line),
    ];
    for line in extras.into_iter().flatten() {
        card = card.push(widget::text::caption(line));
    }

    widget::container(card)
        .padding(12)
        .width(Length::Fill)
        .class(cosmic::theme::Container::Card)
        .into()
}

fn window_view(window: &Window) -> Element<'_, Message> {
    let class = if window.warning {
        cosmic::theme::ProgressBar::Danger
    } else {
        cosmic::theme::ProgressBar::Primary
    };
    let mut column = widget::Column::new()
        .spacing(4)
        .push(widget::text::body(window_line(window)))
        .push(
            progress_bar(0.0..=100.0, percent(window.display_value))
                .girth(6)
                .class(class),
        );
    for text in [&window.reset_text, &window.pace] {
        if !text.is_empty() {
            column = column.push(widget::text::caption(text.as_str()));
        }
    }
    column.into()
}

/// A small round dot in the colour of a service status.
fn dot<'a>(severity: Severity) -> Element<'a, Message> {
    widget::container(widget::Space::new())
        .width(Length::Fixed(STATUS_DOT))
        .height(Length::Fixed(STATUS_DOT))
        .class(cosmic::theme::Container::custom(move |theme| {
            let cosmic = theme.cosmic();
            let color = match severity {
                Severity::Minor => cosmic.warning_color(),
                Severity::Major => cosmic.destructive_color(),
                Severity::Other => cosmic.on_bg_color(),
            };
            widget::container::Style {
                background: Some(cosmic::iced::Color::from(color).into()),
                border: cosmic::iced::Border {
                    radius: (STATUS_DOT / 2.0).into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

#[allow(clippy::cast_possible_truncation)] // A percentage.
fn percent(value: f64) -> f32 {
    value.clamp(0.0, 100.0) as f32
}

/// The popup's keyboard shortcuts: `R` refreshes and `Esc` closes.
pub fn shortcut(event: keyboard::Event) -> Option<Message> {
    let keyboard::Event::KeyPressed {
        key,
        physical_key,
        modifiers,
        repeat,
        ..
    } = event
    else {
        return None;
    };
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    match key {
        Key::Named(Named::Escape) => Some(Message::ClosePopup),
        // By position on non-Latin layouts, as `to_latin` does.
        _ if !repeat && matches!(key.to_latin(physical_key), Some('r' | 'R')) => {
            Some(Message::Refresh)
        }
        _ => None,
    }
}

/// `error`, or "Refreshing…" while busy, or the time of the last update, as in Omarchy's panel.
pub fn status_line(snapshot: &Snapshot) -> String {
    if !snapshot.error.is_empty() {
        snapshot.error.clone()
    } else if snapshot.busy {
        fl!("refreshing")
    } else if snapshot.updated.is_empty() {
        // The first seconds after the desktop app starts, as its own quick view says.
        fl!("waiting-for-usage")
    } else if snapshot.stale {
        fl!("updated-stale", time = snapshot.updated.as_str())
    } else {
        fl!("updated", time = snapshot.updated.as_str())
    }
}

/// Upstream's `providerName`: a few names that capitalising the id gets wrong, or the
/// capitalised id.
pub fn display_name(provider: &str) -> String {
    let name = match provider {
        "codex" => "Codex",
        "claude" => "Claude",
        "copilot" => "GitHub Copilot",
        "gemini" => "Gemini",
        "cursor" => "Cursor",
        "antigravity" => "Antigravity",
        "openrouter" => "OpenRouter",
        "kiro" => "Kiro",
        "" => return fl!("unknown-provider"),
        _ => {
            let mut chars = provider.chars();
            let first = chars.next().into_iter().flat_map(char::to_uppercase);
            return first.chain(chars).collect();
        }
    };
    name.to_owned()
}

/// `{label} · {displayValue}% {displaySuffix}`, as in Omarchy's panel.
pub fn window_line(window: &Window) -> String {
    let suffix = if window.display_suffix.is_empty() {
        "left"
    } else {
        &window.display_suffix
    };
    let value = format!("{}% {suffix}", number(window.display_value));
    if window.label.is_empty() {
        value
    } else {
        format!("{} · {value}", window.label)
    }
}

/// How a service status shows: the colour of its dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// `minor`, or `maintenance`.
    Minor,
    /// `major` or `critical`.
    Major,
    /// Anything else that isn't operational, such as `unknown`.
    Other,
}

/// The entry's status when it isn't operational. A status without text is only a level, for
/// example `unknown` when the status page wasn't checked, and isn't shown.
pub fn status(entry: &Entry) -> Option<(Severity, &str)> {
    let severity = match entry.status_level.as_str() {
        "none" | "" => return None,
        "minor" | "maintenance" => Severity::Minor,
        "major" | "critical" => Severity::Major,
        _ => Severity::Other,
    };
    (!entry.status.is_empty()).then_some((severity, entry.status.as_str()))
}

/// The matching row of the snapshot's spending, which only some providers have.
pub fn spending_for<'a>(snapshot: &'a Snapshot, provider: &str) -> Option<&'a Spending> {
    snapshot
        .spending
        .iter()
        .find(|row| row.provider == provider)
}

/// "Credits: {n} remaining", from the desktop app's quick view.
pub fn credits_line(credits: f64) -> String {
    fl!("credits", amount = number(credits))
}

/// "{period}: {used} / {limit} · Balance {balance}", with only the amounts that are present,
/// as the Mac app's extra usage section. `None` without any amount.
pub fn extra_usage_line(extra: &ExtraUsage) -> Option<String> {
    let spent: Vec<f64> = [extra.used, extra.limit].into_iter().flatten().collect();
    let mut parts = Vec::new();
    if !spent.is_empty() {
        parts.push(amounts(&spent, &extra.currency));
    }
    if let Some(balance) = extra.balance {
        parts.push(fl!(
            "balance",
            amount = amounts(&[balance], &extra.currency)
        ));
    }
    if parts.is_empty() {
        return None;
    }
    let title = if extra.period.is_empty() {
        fl!("extra-usage")
    } else {
        extra.period.clone()
    };
    Some(format!("{title}: {}", parts.join(" · ")))
}

/// "Reset credits: {n} available". `nextExpiresAt` is a UTC timestamp, which the applet doesn't
/// parse (plan §2).
pub fn reset_credits_line(reset: &ResetCredits) -> String {
    fl!("reset-credits", count = number(reset.available))
}

/// "Today {money} · Last 30 days {money}", or the row's error.
pub fn spending_line(spending: &Spending) -> String {
    if spending.error.is_empty() {
        fl!(
            "spending",
            today = money(spending.today),
            month = money(spending.month)
        )
    } else {
        spending.error.clone()
    }
}

/// Upstream's `money`: dollars with two decimals, or "Unavailable".
pub fn money(value: Option<f64>) -> String {
    value.map_or_else(|| fl!("unavailable"), |value| format!("${value:.2}"))
}

/// Amounts separated by `/`. Dollars as [`money`]; anything else, such as `Credits` or `EUR`, as
/// numbers followed once by the currency.
fn amounts(values: &[f64], currency: &str) -> String {
    if currency.is_empty() || currency == "USD" {
        return values
            .iter()
            .map(|&value| money(Some(value)))
            .collect::<Vec<_>>()
            .join(" / ");
    }
    let numbers: Vec<String> = values.iter().map(|&value| number(value)).collect();
    format!("{} {currency}", numbers.join(" / "))
}

/// At most two decimals, without trailing zeros: `3`, `12.5`, `0.33`.
pub fn number(value: f64) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use cosmic::iced::keyboard::{Location, Modifiers, key::Code, key::Physical};

    fn fixture(json: &str) -> Snapshot {
        snapshot::parse(json.as_bytes()).unwrap()
    }

    fn two_providers() -> Snapshot {
        fixture(include_str!(
            "../tests/fixtures/snapshot-two-providers-spending.json"
        ))
    }

    /// Fluent wraps arguments in Unicode isolation marks.
    fn plain(text: &str) -> String {
        text.replace(['\u{2068}', '\u{2069}'], "")
    }

    #[test]
    fn status_line_prefers_error_then_busy_then_update_time() {
        let snapshot = two_providers();
        assert_eq!(plain(&status_line(&snapshot)), "Updated 12:34");

        let stale = Snapshot {
            stale: true,
            ..snapshot.clone()
        };
        assert_eq!(plain(&status_line(&stale)), "Updated 12:34 · older data");

        let busy = Snapshot {
            busy: true,
            ..stale.clone()
        };
        assert_eq!(status_line(&busy), "Refreshing…");

        let error = Snapshot {
            error: "CodexBar CLI not found".into(),
            ..busy
        };
        assert_eq!(status_line(&error), "CodexBar CLI not found");

        assert_eq!(status_line(&Snapshot::default()), "Waiting for usage…");
    }

    #[test]
    fn display_names_follow_upstream() {
        assert_eq!(display_name("claude"), "Claude");
        assert_eq!(display_name("copilot"), "GitHub Copilot");
        assert_eq!(display_name("openrouter"), "OpenRouter");
        assert_eq!(display_name("zed"), "Zed");
        assert_eq!(display_name("ünïcode"), "Ünïcode");
        assert_eq!(display_name(""), "Unknown provider");
    }

    #[test]
    fn window_lines() {
        let snapshot = two_providers();
        let lines: Vec<_> = snapshot.entries[0]
            .windows
            .iter()
            .map(window_line)
            .collect();
        assert_eq!(
            lines,
            [
                "5 hour · 90% left",
                "7 day · 84% left",
                "Fable only · 93% left"
            ]
        );

        let used = fixture(include_str!("../tests/fixtures/snapshot-quota-used.json"));
        assert_eq!(window_line(&used.entries[1].windows[0]), "5 hour · 0% used");

        let bare = Window {
            display_value: 12.5,
            ..Window::default()
        };
        assert_eq!(window_line(&bare), "12.5% left");
    }

    #[test]
    fn progress_is_clamped() {
        assert!((percent(150.0) - 100.0).abs() < f32::EPSILON);
        assert!(percent(-3.0).abs() < f32::EPSILON);
        assert!((percent(42.0) - 42.0).abs() < f32::EPSILON);
    }

    #[test]
    fn status_shows_only_when_not_operational() {
        let snapshot = fixture(include_str!(
            "../tests/fixtures/snapshot-provider-error.json"
        ));
        let [claude, codex, gemini] = &snapshot.entries[..] else {
            panic!("expected three entries");
        };
        assert_eq!(
            status(claude),
            Some((Severity::Minor, "Minor Service Outage"))
        );
        assert_eq!(status(codex), None);
        // `unknown` with no text: the status page wasn't checked.
        assert_eq!(status(gemini), None);

        let outage = Entry {
            status: "Major Outage".into(),
            status_level: "critical".into(),
            ..Entry::default()
        };
        assert_eq!(status(&outage), Some((Severity::Major, "Major Outage")));
        let odd = Entry {
            status: "Unknown".into(),
            status_level: "unknown".into(),
            ..Entry::default()
        };
        assert_eq!(status(&odd), Some((Severity::Other, "Unknown")));
    }

    #[test]
    fn credits_extra_usage_and_reset_credits() {
        let snapshot = two_providers();
        let codex = &snapshot.entries[1];
        assert_eq!(
            plain(&credits_line(codex.credits.unwrap())),
            "Credits: 0 remaining"
        );
        assert_eq!(
            extra_usage_line(codex.extra_usage.as_ref().unwrap()).map(|l| plain(&l)),
            Some("Extra usage: 0 / 0 Credits".into())
        );
        assert_eq!(
            plain(&reset_credits_line(codex.reset_credits.as_ref().unwrap())),
            "Reset credits: 3 available"
        );
        let claude = &snapshot.entries[0];
        assert!(claude.credits.is_none());
        assert!(claude.extra_usage.is_none());
        assert!(claude.reset_credits.is_none());
    }

    #[test]
    fn extra_usage_shows_the_amounts_present() {
        let dollars = ExtraUsage {
            period: "This month".into(),
            currency: "USD".into(),
            used: Some(12.5),
            limit: Some(50.0),
            balance: Some(37.5),
        };
        assert_eq!(
            extra_usage_line(&dollars).map(|l| plain(&l)),
            Some("This month: $12.50 / $50.00 · Balance $37.50".into())
        );
        let balance_only = ExtraUsage {
            currency: "EUR".into(),
            balance: Some(4.333),
            ..ExtraUsage::default()
        };
        assert_eq!(
            extra_usage_line(&balance_only).map(|l| plain(&l)),
            Some("Extra usage: Balance 4.33 EUR".into())
        );
        assert_eq!(extra_usage_line(&ExtraUsage::default()), None);
    }

    #[test]
    fn spending_matches_the_provider() {
        let snapshot = two_providers();
        let claude = spending_for(&snapshot, "claude").unwrap();
        assert_eq!(
            plain(&spending_line(claude)),
            "Today $3.25 · Last 30 days $42.50"
        );
        let codex = spending_for(&snapshot, "codex").unwrap();
        assert_eq!(
            plain(&spending_line(codex)),
            "Today $0.00 · Last 30 days $0.00"
        );
        assert!(spending_for(&snapshot, "gemini").is_none());

        // Without `spending: true` no scan has run yet.
        let plain_poll = fixture(include_str!(
            "../tests/fixtures/snapshot-two-providers.json"
        ));
        assert!(spending_for(&plain_poll, "claude").is_none());

        let unknown_today = Spending {
            today: None,
            month: Some(1.0),
            ..Spending::default()
        };
        assert_eq!(
            plain(&spending_line(&unknown_today)),
            "Today Unavailable · Last 30 days $1.00"
        );
        let failed = Spending {
            error: "Local cost history unavailable".into(),
            ..unknown_today
        };
        assert_eq!(spending_line(&failed), "Local cost history unavailable");
    }

    #[test]
    fn numbers_keep_at_most_two_decimals() {
        assert_eq!(number(0.0), "0");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(3.0), "3");
        assert_eq!(number(12.5), "12.5");
        assert_eq!(number(1.0 / 3.0), "0.33");
        assert_eq!(number(1234.0), "1234");
        assert_eq!(money(Some(3.254)), "$3.25");
        assert_eq!(money(None), "Unavailable");
    }

    fn press(key: Key, code: Code, modifiers: Modifiers) -> keyboard::Event {
        keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: Physical::Code(code),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        }
    }

    #[test]
    fn shortcuts() {
        let none = Modifiers::empty();
        assert!(matches!(
            shortcut(press(Key::Named(Named::Escape), Code::Escape, none)),
            Some(Message::ClosePopup)
        ));
        assert!(matches!(
            shortcut(press(Key::Character("r".into()), Code::KeyR, none)),
            Some(Message::Refresh)
        ));
        assert!(matches!(
            shortcut(press(
                Key::Character("R".into()),
                Code::KeyR,
                Modifiers::SHIFT
            )),
            Some(Message::Refresh)
        ));
        // A Cyrillic layout: the key in R's place.
        assert!(matches!(
            shortcut(press(Key::Character("к".into()), Code::KeyR, none)),
            Some(Message::Refresh)
        ));
        assert!(
            shortcut(press(
                Key::Character("r".into()),
                Code::KeyR,
                Modifiers::CTRL
            ))
            .is_none()
        );
        assert!(shortcut(press(Key::Character("q".into()), Code::KeyQ, none)).is_none());
        let held = keyboard::Event::KeyPressed {
            key: Key::Character("r".into()),
            modified_key: Key::Character("r".into()),
            physical_key: Physical::Code(Code::KeyR),
            location: Location::Standard,
            modifiers: none,
            text: None,
            repeat: true,
        };
        assert!(shortcut(held).is_none());
    }
}
