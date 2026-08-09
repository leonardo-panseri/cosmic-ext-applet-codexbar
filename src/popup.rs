// SPDX-License-Identifier: MPL-2.0

use crate::app::{Message, available_windows, snapshot_percent};
use crate::codexbar::{Provider, Snapshot, Window};
use crate::fl;
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;

pub fn provider_card(
    snapshot: &Snapshot,
    refreshing: bool,
    error: Option<String>,
) -> Element<'_, Message> {
    let mut content = widget::column::with_capacity(12).spacing(12);
    let identity = snapshot.usage.as_ref().map(|usage| &usage.identity);
    let provider = if snapshot.provider.is_empty() {
        identity
            .map(|identity| identity.provider_id.as_str())
            .filter(|provider| !provider.is_empty())
            .unwrap_or("CodexBar")
    } else {
        &snapshot.provider
    };

    let mut header = widget::row::with_capacity(4)
        .push(widget::text::heading(provider))
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
        if !identity.account_email.is_empty() {
            content = content.push(widget::text::body(&identity.account_email));
        }
        if !identity.account_organization.is_empty() {
            content = content.push(widget::text::caption(&identity.account_organization));
        }
    }

    for (label, window) in available_windows(snapshot) {
        content = content.push(window_row(label, window));
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

    if let Some(credits) = &snapshot.credits {
        content = content.push(widget::text::body(format!(
            "Credits · {:.2}",
            credits.remaining
        )));
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
        .padding(16)
        .width(Length::Fill)
        .into()
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

fn window_row(label: &'static str, window: &Window) -> Element<'static, Message> {
    let reset = window
        .reset_description
        .as_deref()
        .or(window.resets_at.as_deref())
        .unwrap_or("Reset unavailable");
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
        .push(widget::determinate_linear(window.used_percent / 100.0).width(Length::Fill))
        .push(widget::text::caption(reset.to_owned()))
        .spacing(4)
        .into()
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
