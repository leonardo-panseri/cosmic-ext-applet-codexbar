// SPDX-License-Identifier: MPL-2.0

//! The popup in the `Connected` state: a header with the status line and the actions as icon
//! buttons, a tab per entry when there's more than one, and the selected entry's page.
//!
//! What the popup says is decided by the pure functions here, so it can be tested against the
//! fixtures. Text from the backend (labels, reset text, pace, plan, status and errors) is shown as
//! it comes (§4.6). Number formats follow upstream's `Integrations/Linux/Shared/Usage.js` and the
//! desktop app's `QuickView.qml`.

use std::rc::Rc;

use cosmic::iced::keyboard::{self, Key, key::Named};
use cosmic::iced::widget::{progress_bar, stack, svg};
use cosmic::iced::{Alignment, Border, Color, Length};
use cosmic::prelude::*;
use cosmic::widget::{self, icon, tooltip};

use crate::app::Message;
use crate::backend;
use crate::guidance::{self, Banner};
use crate::snapshot::{Entry, ExtraUsage, ResetCredits, Snapshot, Spending, Window};
use crate::{fl, icons};

/// The page scrolls beyond this, so the popup stays on screen with many windows.
const PAGE_MAX_HEIGHT: f32 = 560.0;
/// The popup's side margin.
const MARGIN: u16 = 16;
const TAB_WIDTH: f32 = 52.0;
/// Tabs that fit across the popup at [`TAB_WIDTH`]; more scroll sideways.
const TABS_THAT_FIT: usize = 6;
const TAB_LOGO: u16 = 20;
const PAGE_LOGO: u16 = 28;
const DOT: f32 = 8.0;
const BAR_GIRTH: f32 = 8.0;
const PACE_TICK: f32 = 4.0;
/// Inside tooltips, which libcosmic draws tight around their text.
const TOOLTIP_PADDING: f32 = 8.0;
const UNDERLINE: f32 = 2.0;

/// The `Connected` popup for the entry of `provider`, or the first entry. The autostart banner
/// and the tray prompt show under the header ([`crate::guidance`]).
pub fn view<'a>(
    snapshot: &'a Snapshot,
    provider: Option<&str>,
    autostart: Option<Banner<'a>>,
    tray: Option<Banner<'a>>,
) -> Element<'a, Message> {
    let banners: Vec<_> = [
        autostart.map(guidance::autostart_banner),
        tray.map(guidance::tray_banner),
    ]
    .into_iter()
    .flatten()
    .collect();

    let mut popup = widget::Column::new()
        .spacing(12)
        .padding([12, 0, 16, 0])
        .push(header(snapshot));
    if !banners.is_empty() {
        popup = popup.push(
            widget::Column::with_children(banners)
                .spacing(8)
                .padding([0, MARGIN]),
        );
    }
    let entry = selected(snapshot, provider);
    if snapshot.entries.len() > 1 {
        popup = popup.push(tabs(snapshot, entry));
    }
    let page = match entry {
        Some(entry) => page(entry, spending_for(snapshot, &entry.provider)),
        None => empty(snapshot),
    };
    popup
        .push(widget::container(widget::scrollable(page)).max_height(PAGE_MAX_HEIGHT))
        .into()
}

/// The status line, then the actions: Usage & Spend, Settings and Refresh.
fn header(snapshot: &Snapshot) -> Element<'_, Message> {
    let mut status = widget::Row::new().spacing(6).align_y(Alignment::Center);
    if snapshot.stale || !snapshot.error.is_empty() {
        status = status.push(warning_icon(14));
    }
    status = status.push(widget::text::caption(status_line(snapshot)));

    let actions = [
        (
            "utilities-system-monitor-symbolic",
            fl!("usage-and-spend"),
            Some(Message::Open(backend::Window::Spending)),
        ),
        (
            "emblem-system-symbolic",
            fl!("settings"),
            Some(Message::Open(backend::Window::Settings)),
        ),
        (
            "view-refresh-symbolic",
            fl!("refresh"),
            (!snapshot.busy).then_some(Message::Refresh),
        ),
    ];
    let row = actions.into_iter().fold(
        widget::Row::new()
            .spacing(4)
            .align_y(Alignment::Center)
            .push(widget::container(status).width(Length::Fill)),
        |row, (name, label, message)| row.push(action(name, label, message)),
    );
    // The buttons' own padding lines their icons up with the page's margin.
    widget::container(row)
        .padding([0, MARGIN - 8, 0, MARGIN])
        .into()
}

/// An icon button with its label as a tooltip. Below, since the popup ends above the header.
fn action<'a>(name: &'static str, label: String, message: Option<Message>) -> Element<'a, Message> {
    tip(
        widget::button::icon(icon::from_name(name).handle()).on_press_maybe(message),
        widget::text::caption(label),
        tooltip::Position::Bottom,
    )
}

/// A tab per entry, centred, or scrolling sideways when they don't fit, over a divider.
fn tabs<'a>(snapshot: &'a Snapshot, selected: Option<&'a Entry>) -> Element<'a, Message> {
    let row = snapshot
        .entries
        .iter()
        .fold(widget::Row::new().spacing(4), |row, entry| {
            let is_selected = selected.is_some_and(|selected| std::ptr::eq(selected, entry));
            row.push(tab(snapshot, entry, is_selected))
        });
    let strip: Element<'a, Message> = if snapshot.entries.len() > TABS_THAT_FIT {
        widget::scrollable::horizontal(row).into()
    } else {
        widget::container(row).center_x(Length::Fill).into()
    };
    widget::Column::new()
        .push(widget::container(strip).padding([0, MARGIN]))
        .push(widget::container(widget::divider::horizontal::default()).padding([0, MARGIN]))
        .into()
}

/// The provider's logo (or its tag), a dot when it needs attention, and an underline when
/// selected.
fn tab<'a>(snapshot: &'a Snapshot, entry: &'a Entry, selected: bool) -> Element<'a, Message> {
    let logo: Element<'a, Message> = match icons::provider(&entry.provider) {
        Some(logo) => icon::icon(logo).size(TAB_LOGO).into(),
        None => widget::text::heading(tag(snapshot, &entry.provider)).into(),
    };
    let side = f32::from(TAB_LOGO) + 12.0;
    let mut mark = stack![widget::container(logo).center(Length::Fixed(side))];
    if let Some(severity) = attention(entry) {
        mark = mark.push(
            widget::container(dot(severity))
                .align_right(Length::Fill)
                .align_top(Length::Fill),
        );
    }
    let underline = widget::container(widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(UNDERLINE))
        .class(cosmic::theme::Container::custom(move |theme| {
            let color = if selected {
                theme.cosmic().accent_color().into()
            } else {
                Color::TRANSPARENT
            };
            widget::container::Style {
                background: Some(color.into()),
                border: Border {
                    radius: (UNDERLINE / 2.0).into(),
                    ..Border::default()
                },
                ..widget::container::Style::default()
            }
        }));
    let content = widget::Column::new()
        .width(Length::Fill)
        .spacing(4)
        .align_x(Alignment::Center)
        .push(mark)
        .push(underline);
    let button = widget::button::custom(content)
        .width(Length::Fixed(TAB_WIDTH))
        .padding([4, 0, 0, 0])
        .class(cosmic::theme::Button::IconVertical)
        .selected(selected)
        .on_press(Message::SelectProvider(entry.provider.clone()));
    tip(
        button,
        widget::text::caption(display_name(&entry.provider)),
        tooltip::Position::Bottom,
    )
}

/// The entry's title, status and error, then its usage, credits and spending.
fn page<'a>(entry: &'a Entry, spending: Option<&'a Spending>) -> Element<'a, Message> {
    let mut page = widget::Column::new()
        .spacing(16)
        .padding([4, MARGIN, 0, MARGIN])
        .push(title(entry));

    if let Some((severity, status)) = status(entry) {
        page = page.push(
            widget::Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(dot(severity))
                .push(widget::text::body(status)),
        );
    }
    if !entry.error.is_empty() {
        page = page.push(
            widget::Row::new()
                .spacing(8)
                .push(warning_icon(16))
                .push(widget::text::body(entry.error.as_str()).width(Length::Fill)),
        );
    }

    if !entry.windows.is_empty() {
        let windows = entry
            .windows
            .iter()
            .fold(widget::Column::new().spacing(16), |column, window| {
                column.push(window_view(window))
            });
        page = page.push(section(fl!("usage"), windows));
    }

    let credits: Vec<_> = [
        entry.credits.map(credits_row),
        entry.extra_usage.as_ref().and_then(extra_usage_row),
        entry.reset_credits.as_ref().map(reset_credits_row),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !credits.is_empty() {
        page = page.push(section(fl!("credits"), rows(credits)));
    }

    if let Some(spending) = spending {
        let content: Element<'a, Message> = if spending.error.is_empty() {
            rows(spending_rows(spending))
        } else {
            widget::text::body(spending.error.as_str()).into()
        };
        page = page.push(section(fl!("spending"), content));
    }
    page.into()
}

/// The page without entries: nothing while the desktop app is starting, since the status line
/// says so, and otherwise where to turn providers on.
fn empty(snapshot: &Snapshot) -> Element<'_, Message> {
    let mut page = widget::Column::new().padding([0, MARGIN]);
    if !snapshot.updated.is_empty() {
        page = page.push(widget::text::body(fl!("no-providers")));
    }
    page.into()
}

/// Logo, name and plan.
fn title(entry: &Entry) -> Element<'_, Message> {
    let mut row = widget::Row::new().spacing(12).align_y(Alignment::Center);
    if let Some(logo) = icons::provider(&entry.provider) {
        row = row.push(icon::icon(logo).size(PAGE_LOGO));
    }
    row = row.push(widget::text::title3(display_name(&entry.provider)).width(Length::Fill));
    if !entry.plan.is_empty() {
        row = row.push(plan_badge(plan_name(&entry.plan)));
    }
    row.into()
}

/// The plan in an accent-coloured pill.
fn plan_badge<'a>(plan: String) -> Element<'a, Message> {
    widget::container(widget::text::caption(plan))
        .padding([2, 10])
        .class(cosmic::theme::Container::custom(|theme| {
            let accent: Color = theme.cosmic().accent_text_color().into();
            widget::container::Style {
                text_color: Some(accent),
                border: Border {
                    radius: 999.0.into(),
                    width: 1.0,
                    color: accent,
                },
                ..widget::container::Style::default()
            }
        }))
        .into()
}

/// A heading over a card.
fn section<'a>(heading: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    widget::Column::new()
        .spacing(8)
        .push(widget::text::heading(heading))
        .push(
            widget::container(content)
                .padding(16)
                .width(Length::Fill)
                .class(cosmic::theme::Container::Card),
        )
        .into()
}

/// Labels on the left, values on the right.
fn rows<'a>(rows: Vec<(String, String)>) -> Element<'a, Message> {
    rows.into_iter()
        .fold(
            widget::Column::new().spacing(8),
            |column, (label, value)| {
                column.push(
                    widget::Row::new()
                        .spacing(12)
                        .push(widget::text::body(label).width(Length::Fill))
                        .push(widget::text::body(value)),
                )
            },
        )
        .into()
}

/// The label, the bar, then the usage and the reset time on one line.
fn window_view(window: &Window) -> Element<'_, Message> {
    let mut column = widget::Column::new().spacing(6);
    if !window.label.is_empty() {
        column = column.push(widget::text::body(window.label.as_str()));
    }
    let mut footer = widget::Row::new()
        .spacing(8)
        .push(widget::text::caption(usage_text(window)).width(Length::Fill));
    if !window.reset_text.is_empty() {
        footer = footer.push(widget::text::caption(window.reset_text.as_str()));
    }
    column.push(bar(window)).push(footer).into()
}

/// The progress bar, with a tick where usage is expected to be by now and the pace as a
/// tooltip.
fn bar(window: &Window) -> Element<'_, Message> {
    let class = if window.warning {
        cosmic::theme::ProgressBar::Danger
    } else {
        cosmic::theme::ProgressBar::Primary
    };
    let progress = progress_bar(0.0..=100.0, percent(window.display_value))
        .girth(BAR_GIRTH)
        .class(class);
    let bar: Element<'_, Message> = match pace_marker(window) {
        None => progress.into(),
        Some(at) => {
            let before = portion(at);
            let tick = widget::container(widget::Space::new())
                .width(Length::Fixed(PACE_TICK))
                .height(Length::Fill)
                .class(cosmic::theme::Container::custom(|theme| {
                    let mut color: Color = theme.cosmic().on_bg_color().into();
                    color.a = 0.7;
                    widget::container::Style {
                        background: Some(color.into()),
                        ..widget::container::Style::default()
                    }
                }));
            stack![
                progress,
                widget::Row::new()
                    .height(Length::Fixed(BAR_GIRTH))
                    .push(widget::Space::new().width(Length::FillPortion(before)))
                    .push(tick)
                    .push(widget::Space::new().width(Length::FillPortion(1000 - before))),
            ]
            .into()
        }
    };
    let lines: Vec<String> = match pace_lines(window) {
        lines if lines.is_empty() => vec![fl!("no-pace")],
        lines => lines.into_iter().map(str::to_owned).collect(),
    };
    let pace = lines
        .into_iter()
        .fold(widget::Column::new().spacing(2), |column, line| {
            column.push(widget::text::caption(line))
        });
    tip(bar, pace, tooltip::Position::Top)
}

/// A tooltip with room around its text.
fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    text: impl Into<Element<'a, Message>>,
    position: tooltip::Position,
) -> Element<'a, Message> {
    tooltip(content, text, position)
        .padding(TOOLTIP_PADDING)
        .gap(4)
        .into()
}

/// A small round dot in the colour of a severity.
fn dot<'a>(severity: Severity) -> Element<'a, Message> {
    widget::container(widget::Space::new())
        .width(Length::Fixed(DOT))
        .height(Length::Fixed(DOT))
        .class(cosmic::theme::Container::custom(move |theme| {
            let cosmic = theme.cosmic();
            let color = match severity {
                Severity::Minor => cosmic.warning_color(),
                Severity::Major => cosmic.destructive_color(),
                Severity::Other => cosmic.on_bg_color(),
            };
            widget::container::Style {
                background: Some(Color::from(color).into()),
                border: Border {
                    radius: (DOT / 2.0).into(),
                    ..Border::default()
                },
                ..widget::container::Style::default()
            }
        }))
        .into()
}

fn warning_icon<'a>(size: u16) -> Element<'a, Message> {
    icon::from_name("dialog-warning-symbolic")
        .size(size)
        .icon()
        .class(cosmic::theme::Svg::Custom(Rc::new(|theme| svg::Style {
            color: Some(theme.cosmic().warning_color().into()),
        })))
        .into()
}

#[allow(clippy::cast_possible_truncation)] // A percentage.
fn percent(value: f64) -> f32 {
    value.clamp(0.0, 100.0) as f32
}

/// A percentage in thousandths, for [`Length::FillPortion`].
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0 to 1000.
fn portion(percent: f32) -> u16 {
    (percent.clamp(0.0, 100.0) * 10.0).round() as u16
}

/// Which way the arrow keys move between tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Previous,
    Next,
}

/// The popup's keyboard shortcuts: `R` refreshes, `Esc` closes, and the left and right arrows
/// switch tabs.
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
        Key::Named(Named::ArrowLeft) => Some(Message::SwitchProvider(Step::Previous)),
        Key::Named(Named::ArrowRight) => Some(Message::SwitchProvider(Step::Next)),
        // By position on non-Latin layouts, as `to_latin` does.
        _ if !repeat && matches!(key.to_latin(physical_key), Some('r' | 'R')) => {
            Some(Message::Refresh)
        }
        _ => None,
    }
}

/// The entry of `provider`, or the first entry when that provider is gone or none was chosen.
pub fn selected<'a>(snapshot: &'a Snapshot, provider: Option<&str>) -> Option<&'a Entry> {
    provider
        .and_then(|provider| {
            snapshot
                .entries
                .iter()
                .find(|entry| entry.provider == provider)
        })
        .or_else(|| snapshot.entries.first())
}

/// The provider one tab before or after `provider`'s, wrapping around.
pub fn adjacent<'a>(snapshot: &'a Snapshot, provider: Option<&str>, step: Step) -> Option<&'a str> {
    let entries = &snapshot.entries;
    let count = entries.len();
    if count == 0 {
        return None;
    }
    let current = provider
        .and_then(|provider| entries.iter().position(|entry| entry.provider == provider))
        .unwrap_or(0);
    let next = match step {
        Step::Previous => (current + count - 1) % count,
        Step::Next => (current + 1) % count,
    };
    Some(&entries[next].provider)
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

/// What a tab shows for a provider without a logo: its bar entry's tag, or the first two letters
/// of its name.
pub fn tag(snapshot: &Snapshot, provider: &str) -> String {
    snapshot
        .bar_entries
        .iter()
        .find(|bar| bar.provider == provider && !bar.tag.is_empty())
        .map_or_else(
            || display_name(provider).chars().take(2).collect(),
            |bar| bar.tag.clone(),
        )
}

/// The plan with a capital first letter: Codex sends `plus`, which looks like a typo next to
/// Claude's `Claude Pro`. The only backend text the applet changes (plan §4.6).
pub fn plan_name(plan: &str) -> String {
    let mut chars = plan.chars();
    let first = chars.next().into_iter().flat_map(char::to_uppercase);
    first.chain(chars).collect()
}

/// `{displayValue}% {displaySuffix}`, as in Omarchy's panel.
pub fn usage_text(window: &Window) -> String {
    let suffix = if window.display_suffix.is_empty() {
        "left"
    } else {
        &window.display_suffix
    };
    format!("{}% {suffix}", number(window.display_value))
}

/// Where the pace tick goes on the bar: the usage expected by now, as a share of the bar. The
/// bar shows what's left unless `displaySuffix` is `used`.
pub fn pace_marker(window: &Window) -> Option<f32> {
    let expected = window.pace_expected?;
    let at = if window.display_suffix == "used" {
        expected
    } else {
        100.0 - expected
    };
    Some(percent(at))
}

/// The pace text one part per line; the tooltip says there's no pace when this is empty, split where upstream joins them with `|`.
pub fn pace_lines(window: &Window) -> Vec<&str> {
    window
        .pace
        .split('|')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

/// How a service status or a tab's dot shows: its colour. Ordered from least to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Anything that isn't operational and isn't a known level, such as `unknown`.
    Other,
    /// `minor`, or `maintenance`.
    Minor,
    /// `major` or `critical`; on a tab, also an error or a window running low.
    Major,
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

/// Whether a tab gets a dot, and its colour: the entry's error or a window with a warning count
/// as major, otherwise its status.
pub fn attention(entry: &Entry) -> Option<Severity> {
    let failing = !entry.error.is_empty() || entry.windows.iter().any(|window| window.warning);
    let status = status(entry).map(|(severity, _)| severity);
    failing.then_some(Severity::Major).max(status)
}

/// The matching row of the snapshot's spending, which only some providers have.
pub fn spending_for<'a>(snapshot: &'a Snapshot, provider: &str) -> Option<&'a Spending> {
    snapshot
        .spending
        .iter()
        .find(|row| row.provider == provider)
}

/// "Remaining" and the credits left, from the desktop app's quick view.
pub fn credits_row(credits: f64) -> (String, String) {
    (fl!("credits-remaining"), number(credits))
}

/// The period, and "{used} / {limit} · Balance {balance}" with only the amounts that are
/// present, as the Mac app's extra usage section. `None` without any amount.
pub fn extra_usage_row(extra: &ExtraUsage) -> Option<(String, String)> {
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
    Some((title, parts.join(" · ")))
}

/// "Reset credits" and "{n} available". `nextExpiresAt` is a UTC timestamp, which the applet
/// doesn't parse (plan §2).
pub fn reset_credits_row(reset: &ResetCredits) -> (String, String) {
    (
        fl!("reset-credits"),
        fl!("reset-credits-available", count = number(reset.available)),
    )
}

/// Today's and the last 30 days' spending. The page shows a row's error instead.
pub fn spending_rows(spending: &Spending) -> Vec<(String, String)> {
    vec![
        (fl!("spending-today"), money(spending.today)),
        (fl!("spending-month"), money(spending.month)),
    ]
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

    fn plain_row((label, value): (String, String)) -> (String, String) {
        (plain(&label), plain(&value))
    }

    fn row(label: &str, value: &str) -> (String, String) {
        (label.to_owned(), value.to_owned())
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
    fn the_chosen_provider_is_selected_or_else_the_first() {
        let snapshot = two_providers();
        let provider = |chosen| selected(&snapshot, chosen).map(|entry| entry.provider.as_str());
        assert_eq!(provider(None), Some("claude"));
        assert_eq!(provider(Some("codex")), Some("codex"));
        // Turned off in the desktop app since it was chosen.
        assert_eq!(provider(Some("gemini")), Some("claude"));
        assert!(selected(&Snapshot::default(), Some("claude")).is_none());
    }

    #[test]
    fn arrows_move_between_tabs_and_wrap_around() {
        let snapshot = fixture(include_str!(
            "../tests/fixtures/snapshot-provider-error.json"
        ));
        let step = |from, step| adjacent(&snapshot, from, step);
        assert_eq!(step(None, Step::Next), Some("codex"));
        assert_eq!(step(Some("codex"), Step::Next), Some("gemini"));
        assert_eq!(step(Some("gemini"), Step::Next), Some("claude"));
        assert_eq!(step(Some("claude"), Step::Previous), Some("gemini"));
        assert_eq!(step(Some("gone"), Step::Previous), Some("gemini"));
        assert_eq!(adjacent(&Snapshot::default(), None, Step::Next), None);
    }

    #[test]
    fn tabs_without_a_logo_show_the_tag_or_initials() {
        let snapshot = two_providers();
        assert_eq!(tag(&snapshot, "claude"), "CL");
        assert_eq!(tag(&snapshot, "zed"), "Ze");
    }

    #[test]
    fn plans_start_with_a_capital() {
        assert_eq!(plan_name("plus"), "Plus");
        assert_eq!(plan_name("Claude Pro"), "Claude Pro");
        assert_eq!(plan_name("ünïcode"), "Ünïcode");
        assert_eq!(plan_name(""), "");
    }

    #[test]
    fn usage_texts() {
        let snapshot = two_providers();
        let texts: Vec<_> = snapshot.entries[0]
            .windows
            .iter()
            .map(|window| (window.label.as_str(), usage_text(window)))
            .collect();
        assert_eq!(
            texts,
            [
                ("5 hour", "90% left".to_owned()),
                ("7 day", "84% left".to_owned()),
                ("Fable only", "93% left".to_owned())
            ]
        );

        let used = fixture(include_str!("../tests/fixtures/snapshot-quota-used.json"));
        assert_eq!(usage_text(&used.entries[1].windows[0]), "0% used");

        let bare = Window {
            display_value: 12.5,
            ..Window::default()
        };
        assert_eq!(usage_text(&bare), "12.5% left");
    }

    #[test]
    fn the_pace_tick_follows_what_the_bar_shows() {
        let snapshot = two_providers();
        let [five_hour, seven_day, fable] = &snapshot.entries[0].windows[..] else {
            panic!("expected three windows");
        };
        // Expected 98% and 46% used, on bars that show what's left.
        assert_eq!(pace_marker(five_hour), Some(2.0));
        assert_eq!(pace_marker(seven_day), Some(54.0));
        assert_eq!(pace_marker(fable), None);

        let used = Window {
            display_suffix: "used".into(),
            pace_expected: Some(30.0),
            ..Window::default()
        };
        assert_eq!(pace_marker(&used), Some(30.0));
        let over = Window {
            pace_expected: Some(140.0),
            ..used
        };
        assert_eq!(pace_marker(&over), Some(100.0));
        assert_eq!(portion(54.0), 540);
        assert_eq!(portion(-1.0), 0);
    }

    #[test]
    fn pace_shows_one_part_per_line() {
        let snapshot = two_providers();
        assert_eq!(
            pace_lines(&snapshot.entries[0].windows[0]),
            ["88% in reserve", "Expected 98% used", "Lasts until reset"]
        );
        assert!(pace_lines(&snapshot.entries[0].windows[2]).is_empty());
        let single = Window {
            pace: "On track".into(),
            ..Window::default()
        };
        assert_eq!(pace_lines(&single), ["On track"]);
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
    fn tabs_flag_errors_low_windows_and_outages() {
        let snapshot = fixture(include_str!(
            "../tests/fixtures/snapshot-provider-error.json"
        ));
        let [claude, codex, gemini] = &snapshot.entries[..] else {
            panic!("expected three entries");
        };
        assert_eq!(attention(claude), Some(Severity::Minor));
        assert_eq!(attention(codex), None);
        assert_eq!(attention(gemini), Some(Severity::Major));

        let low = Entry {
            status: "Minor Service Outage".into(),
            status_level: "minor".into(),
            windows: vec![Window {
                warning: true,
                ..Window::default()
            }],
            ..Entry::default()
        };
        assert_eq!(attention(&low), Some(Severity::Major));
    }

    #[test]
    fn credits_extra_usage_and_reset_credits() {
        let snapshot = two_providers();
        let codex = &snapshot.entries[1];
        assert_eq!(
            plain_row(credits_row(codex.credits.unwrap())),
            row("Remaining", "0")
        );
        assert_eq!(
            extra_usage_row(codex.extra_usage.as_ref().unwrap()).map(plain_row),
            Some(row("Extra usage", "0 / 0 Credits"))
        );
        assert_eq!(
            plain_row(reset_credits_row(codex.reset_credits.as_ref().unwrap())),
            row("Reset credits", "3 available")
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
            extra_usage_row(&dollars).map(plain_row),
            Some(row("This month", "$12.50 / $50.00 · Balance $37.50"))
        );
        let balance_only = ExtraUsage {
            currency: "EUR".into(),
            balance: Some(4.333),
            ..ExtraUsage::default()
        };
        assert_eq!(
            extra_usage_row(&balance_only).map(plain_row),
            Some(row("Extra usage", "Balance 4.33 EUR"))
        );
        assert_eq!(extra_usage_row(&ExtraUsage::default()), None);
    }

    #[test]
    fn spending_matches_the_provider() {
        let snapshot = two_providers();
        let rows =
            |spending| -> Vec<_> { spending_rows(spending).into_iter().map(plain_row).collect() };
        let claude = spending_for(&snapshot, "claude").unwrap();
        assert_eq!(
            rows(claude),
            [row("Today", "$3.25"), row("Last 30 days", "$42.50")]
        );
        let codex = spending_for(&snapshot, "codex").unwrap();
        assert_eq!(
            rows(codex),
            [row("Today", "$0.00"), row("Last 30 days", "$0.00")]
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
            rows(&unknown_today),
            [row("Today", "Unavailable"), row("Last 30 days", "$1.00")]
        );
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
        assert!(matches!(
            shortcut(press(Key::Named(Named::ArrowLeft), Code::ArrowLeft, none)),
            Some(Message::SwitchProvider(Step::Previous))
        ));
        assert!(matches!(
            shortcut(press(Key::Named(Named::ArrowRight), Code::ArrowRight, none)),
            Some(Message::SwitchProvider(Step::Next))
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
