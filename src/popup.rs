// SPDX-License-Identifier: MPL-2.0

use crate::app::{Message, available_windows, snapshot_percent};
use crate::codexbar::{Identity, Provider, ProviderStorage, Snapshot, Window};
use crate::config::{Config, PresentationLayout, ResetDisplay, UsageBarFill};
use crate::fl;
use crate::privacy;
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;

pub fn provider_card<'a>(
    snapshot: &'a Snapshot,
    refreshing: bool,
    error: Option<String>,
    config: &'a Config,
) -> Element<'a, Message> {
    let (padding, spacing) = match config.presentation_layout {
        PresentationLayout::Compact => (8, 8),
        PresentationLayout::Balanced => (16, 12),
        PresentationLayout::Detailed => (20, 16),
    };
    let mut content = widget::column::with_capacity(12).spacing(spacing);
    let identity = snapshot.usage.as_ref().map(|usage| &usage.identity);
    let provider = if snapshot.provider.is_empty() {
        identity
            .map(|identity| identity.provider_id.as_str())
            .filter(|provider| !provider.is_empty())
            .unwrap_or("CodexBar")
    } else {
        &snapshot.provider
    };

    let provider_heading: Element<'_, Message> = if config.high_contrast {
        widget::text::title3(provider).into()
    } else {
        widget::text::heading(provider).into()
    };
    let mut header = widget::row::with_capacity(4)
        .push(provider_heading)
        .push(widget::space::horizontal())
        .align_y(Alignment::Center);
    if let Some(plan) = identity
        .map(|identity| identity.login_method.as_str())
        .filter(|plan| !plan.is_empty())
    {
        header =
            header.push(widget::container(widget::text::caption_heading(plan)).padding([4, 8]));
    }
    header = header.push(
        widget::button::text(if refreshing {
            fl!("refreshing-usage")
        } else {
            fl!("refresh-usage")
        })
        .on_press_maybe((!refreshing).then_some(Message::RefreshRequested)),
    );
    content = content.push(header);

    if let Some(identity) = identity {
        let [email, organization] = identity_lines(identity, config.hide_personal_info);
        if !email.is_empty() {
            content = content.push(widget::text::body(email));
        }
        if !organization.is_empty() {
            content = content.push(widget::text::caption(organization));
        }
    }

    let thresholds = config.quota_thresholds.for_provider(&snapshot.provider);
    for (label, window) in available_windows(snapshot) {
        let threshold = match label {
            "Session" => thresholds.primary,
            "Weekly" => thresholds.secondary,
            _ => thresholds.tertiary,
        };
        content = content.push(window_row(label, window, config, threshold));
    }

    if let Some(summary) = snapshot.pace.as_ref().and_then(|pace| {
        [&pace.primary, &pace.secondary, &pace.tertiary]
            .into_iter()
            .flatten()
            .map(|pace| pace.summary.as_str())
            .find(|summary| !summary.is_empty())
    }) {
        content = content.push(widget::text::body(format!("Pace · {summary}")));
    }

    if config.show_credits
        && let Some(credits) = &snapshot.credits
    {
        content = content.push(widget::text::body(format!(
            "Credits · {:.2}",
            credits.remaining
        )));
    }

    if let Some(storage) = visible_storage(snapshot, config.show_storage_usage) {
        content = content.push(widget::text::body(format!(
            "{} · {}",
            fl!("provider-storage"),
            format_bytes(storage.total_bytes)
        )));
        let rows = storage_rows(storage);
        if !rows.is_empty() {
            content = content.push(widget::text::caption(fl!("provider-storage-copy")));
        }
        for (path, bytes) in rows {
            let label = bytes.map_or_else(
                || privacy::redact(path, config.hide_personal_info).to_owned(),
                |bytes| {
                    format!(
                        "{} · {}",
                        privacy::redact(path, config.hide_personal_info),
                        format_bytes(bytes)
                    )
                },
            );
            content = if config.hide_personal_info {
                content.push(widget::text::caption(label))
            } else {
                content
                    .push(widget::button::text(label).on_press(Message::CopyText(path.to_owned())))
            };
        }
    }

    if config.show_changelog_links
        && let Some(url) = changelog_url(&snapshot.provider)
    {
        content = content.push(
            widget::button::text(fl!("provider-changelog"))
                .on_press(Message::OpenLink(url.to_owned())),
        );
    }

    if let Some(status) = &snapshot.status {
        let detail = if status.description.is_empty() {
            &status.indicator
        } else {
            &status.description
        };
        if !detail.is_empty() {
            content = content.push(widget::text::body(format!("Status · {detail}")));
        }
    }

    if let Some(error) = error {
        content = content
            .push(widget::divider::horizontal::light())
            .push(widget::text::heading(fl!("refresh-error")))
            .push(widget::text::body(error))
            .push(widget::button::text(fl!("copy-error")).on_press(Message::CopyError));
    }

    widget::container(content)
        .padding(padding)
        .width(Length::Fill)
        .into()
}

fn identity_lines(identity: &Identity, hide_personal_info: bool) -> [&str; 2] {
    [
        privacy::redact(&identity.account_email, hide_personal_info),
        privacy::redact(&identity.account_organization, hide_personal_info),
    ]
}

fn visible_storage(snapshot: &Snapshot, enabled: bool) -> Option<&ProviderStorage> {
    enabled.then_some(snapshot.storage.as_ref()).flatten()
}

fn storage_rows(storage: &ProviderStorage) -> Vec<(&str, Option<u64>)> {
    if storage.components.is_empty() {
        storage
            .paths
            .iter()
            .map(|path| (path.as_str(), None))
            .collect()
    } else {
        storage
            .components
            .iter()
            .map(|component| (component.path.as_str(), Some(component.total_bytes)))
            .collect()
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn changelog_url(provider: &str) -> Option<&'static str> {
    match provider {
        "codex" | "openai" => Some("https://developers.openai.com/codex/changelog"),
        "claude" => Some("https://docs.anthropic.com/en/release-notes/overview"),
        "cursor" => Some("https://cursor.com/changelog"),
        "copilot" => Some("https://github.blog/changelog/label/copilot/"),
        "gemini" => {
            Some("https://developers.google.com/gemini-code-assist/resources/release-notes")
        }
        "kiro" => Some("https://kiro.dev/changelog/"),
        _ => None,
    }
}

pub fn overview<'a>(
    snapshots: Vec<&'a Snapshot>,
    providers: &'a [Provider],
) -> Element<'a, Message> {
    let mut rows = widget::column::with_capacity(snapshots.len()).spacing(10);
    if snapshots.is_empty() {
        rows = rows.push(widget::text::body(fl!("loading-usage")));
    }
    for snapshot in snapshots {
        let label = providers
            .iter()
            .find(|provider| provider.id == snapshot.provider)
            .map(|provider| provider.display_name.as_str())
            .filter(|label| !label.is_empty())
            .unwrap_or(&snapshot.provider);
        let percent = snapshot_percent(snapshot);
        rows = rows.push(
            widget::column::with_capacity(2)
                .push(
                    widget::row::with_capacity(2)
                        .push(widget::text::body(label))
                        .push(widget::space::horizontal())
                        .push(widget::text::body(format!("{percent:.0}%"))),
                )
                .push(widget::determinate_linear(percent / 100.0).width(Length::Fill))
                .spacing(4),
        );
    }

    widget::container(rows)
        .padding([4, 16, 16, 16])
        .width(Length::Fill)
        .into()
}

fn window_row(
    label: &'static str,
    window: &Window,
    config: &Config,
    threshold: u8,
) -> Element<'static, Message> {
    let reset = reset_label(window, config.reset_display);
    let title = if window.window_minutes == 0 {
        label.to_owned()
    } else {
        format!("{label} · {}", window_duration(window.window_minutes))
    };

    widget::column::with_capacity(3)
        .push(
            widget::row::with_capacity(2)
                .push(widget::text::body(title))
                .push(widget::space::horizontal())
                .push(widget::text::body(format!("{:.0}%", window.used_percent))),
        )
        .push(
            widget::determinate_linear(window.used_percent / 100.0)
                .markers(progress_markers(
                    config.usage_bar_fill,
                    config.warning_markers.then_some(threshold),
                ))
                .girth(if config.high_contrast { 8.0 } else { 4.0 })
                .width(Length::Fill),
        )
        .push(widget::text::caption(reset.to_owned()))
        .spacing(4)
        .into()
}

fn reset_label(window: &Window, display: ResetDisplay) -> &str {
    match display {
        ResetDisplay::Countdown => window
            .reset_description
            .as_deref()
            .or(window.resets_at.as_deref()),
        ResetDisplay::Clock => window
            .resets_at
            .as_deref()
            .or(window.reset_description.as_deref()),
    }
    .unwrap_or("Reset unavailable")
}

fn progress_markers(fill: UsageBarFill, warning: Option<u8>) -> Vec<f32> {
    let mut markers = if fill == UsageBarFill::Segmented {
        vec![0.25, 0.5, 0.75]
    } else {
        Vec::new()
    };
    if let Some(warning) = warning {
        markers.push(f32::from(warning) / 100.0);
    }
    markers.sort_by(f32::total_cmp);
    markers.dedup();
    markers
}

fn window_duration(minutes: u32) -> String {
    if minutes.is_multiple_of(10_080) {
        format!("{}w", minutes / 10_080)
    } else if minutes.is_multiple_of(1_440) {
        format!("{}d", minutes / 1_440)
    } else if minutes.is_multiple_of(60) {
        format!("{}h", minutes / 60)
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codexbar::{Identity, ProviderStorage, StorageComponent};

    #[test]
    fn privacy_redacts_identity_without_mutating_it() {
        let identity = Identity {
            account_email: "dev@example.com".into(),
            account_organization: "Secret Org".into(),
            ..Default::default()
        };

        assert_eq!(
            identity_lines(&identity, false),
            ["dev@example.com", "Secret Org"]
        );
        assert_eq!(
            identity_lines(&identity, true),
            [crate::privacy::PRIVATE_VALUE, crate::privacy::PRIVATE_VALUE]
        );
        assert_eq!(identity.account_email, "dev@example.com");
    }

    #[test]
    fn storage_is_only_displayed_when_enabled_and_present() {
        let storage = ProviderStorage {
            total_bytes: 1_536,
            components: vec![StorageComponent {
                path: "/home/dev/.codex".into(),
                total_bytes: 1_536,
            }],
            ..Default::default()
        };
        let snapshot = Snapshot {
            storage: Some(storage),
            ..Default::default()
        };

        assert!(visible_storage(&snapshot, false).is_none());
        assert_eq!(visible_storage(&snapshot, true).unwrap().total_bytes, 1_536);
        assert_eq!(format_bytes(1_536), "1.5 KiB");
    }

    #[test]
    fn storage_rows_fall_back_to_paths_without_components() {
        let storage = ProviderStorage {
            total_bytes: 42,
            paths: vec!["/home/dev/.codex".into(), "/home/dev/.cache/codex".into()],
            ..Default::default()
        };

        assert_eq!(
            storage_rows(&storage),
            vec![("/home/dev/.codex", None), ("/home/dev/.cache/codex", None)]
        );
    }

    #[test]
    fn known_changelog_urls_are_https() {
        for provider in ["codex", "claude", "cursor", "copilot", "gemini", "kiro"] {
            assert!(crate::settings::validated_https_url(changelog_url(provider).unwrap()).is_ok());
        }
        assert!(changelog_url("unknown").is_none());
    }

    #[test]
    fn menu_presentation_changes_reset_and_fill_rendering() {
        let window = Window {
            resets_at: Some("2026-08-10T09:00:00Z".into()),
            reset_description: Some("in 2 hours".into()),
            ..Default::default()
        };

        assert_eq!(reset_label(&window, ResetDisplay::Countdown), "in 2 hours");
        assert_eq!(
            reset_label(&window, ResetDisplay::Clock),
            "2026-08-10T09:00:00Z"
        );
        assert!(progress_markers(UsageBarFill::Solid, None).is_empty());
        assert_eq!(progress_markers(UsageBarFill::Segmented, None).len(), 3);
        assert_eq!(progress_markers(UsageBarFill::Solid, Some(80)), vec![0.8]);
    }
}
