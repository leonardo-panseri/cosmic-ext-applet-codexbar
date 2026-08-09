// SPDX-License-Identifier: MPL-2.0

use crate::codexbar::{self, Snapshot, Window};
use crate::config::{Config, IconStyle};
use crate::fl;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::{
    Alignment, Length, Limits, Subscription,
    alignment::{Horizontal, Vertical},
    clipboard,
    widget::Stack,
    window::Id,
};
use cosmic::prelude::*;
use cosmic::widget;

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
#[derive(Default)]
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// The popup id.
    popup: Option<Id>,
    /// Configuration data that persists between application runs.
    config: Config,
    /// Last successfully loaded all-provider snapshot.
    snapshots: Vec<Snapshot>,
    /// The latest refresh failure; snapshots remain visible while this is set.
    refresh_error: Option<String>,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    SnapshotsLoaded(Result<Vec<Snapshot>, String>),
    UpdateConfig(Config),
    CopyError,
}

impl AppModel {
    fn apply_snapshot_result(&mut self, result: Result<Vec<Snapshot>, String>) {
        match result {
            Ok(snapshots) => {
                self.snapshots = snapshots;
                self.refresh_error = None;
            }
            Err(error) => self.refresh_error = Some(error),
        }
    }

    fn selected_snapshot(&self) -> Option<&Snapshot> {
        self.snapshots
            .iter()
            .find(|snapshot| snapshot.usage.is_some())
            .or_else(|| self.snapshots.first())
    }

    fn error_detail(&self) -> Option<String> {
        self.refresh_error.clone().or_else(|| {
            let snapshot = self
                .snapshots
                .iter()
                .find(|snapshot| snapshot.error.is_some())?;
            let error = snapshot.error.as_ref()?;
            let source = if snapshot.provider.is_empty() {
                &error.kind
            } else {
                &snapshot.provider
            };
            Some(if source.is_empty() {
                error.message.clone()
            } else {
                format!("{source}: {}", error.message)
            })
        })
    }

    fn provider_card<'a>(&'a self, snapshot: &'a Snapshot) -> Element<'a, Message> {
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

        let mut header = widget::row::with_capacity(3)
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

        if let Some(error) = self.error_detail() {
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

    fn panel_button(&self, label: Option<String>, has_error: bool) -> Element<'_, Message> {
        let (icon_width, icon_height) = self.core.applet.suggested_size(true);
        let icon = indicator_icon(has_error, icon_width);
        let Some(label) = label else {
            return self
                .core
                .applet
                .button_from_element(icon, true)
                .on_press(Message::TogglePopup)
                .into();
        };

        let (major_padding, minor_padding) = self.core.applet.suggested_padding(true);
        let (horizontal_padding, vertical_padding) = if self.core.applet.is_horizontal() {
            (major_padding, minor_padding)
        } else {
            (minor_padding, major_padding)
        };
        let content = widget::row::with_capacity(2)
            .push(icon)
            .push(widget::text(label))
            .align_y(Alignment::Center)
            .spacing(4);

        widget::button::custom(content)
            .on_press_down(Message::TogglePopup)
            .height(Length::Fixed(f32::from(icon_height + 2 * vertical_padding)))
            .padding([vertical_padding, horizontal_padding])
            .class(cosmic::theme::Button::AppletIcon)
            .into()
    }
}

fn indicator_icon(has_error: bool, size: u16) -> Element<'static, Message> {
    let icon =
        widget::icon::from_svg_bytes(&include_bytes!("../resources/icon.svg")[..]).symbolic(true);
    let icon = widget::icon(icon)
        .size(size)
        .opacity(if has_error { 0.45 } else { 1.0 });

    if has_error {
        Stack::new()
            .push(icon)
            .push(
                widget::container(widget::text::caption_heading("!"))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Horizontal::Right)
                    .align_y(Vertical::Top),
            )
            .width(Length::Fixed(f32::from(size)))
            .height(Length::Fixed(f32::from(size)))
            .into()
    } else {
        icon.into()
    }
}

fn available_windows(snapshot: &Snapshot) -> impl Iterator<Item = (&'static str, &Window)> {
    let usage = snapshot.usage.as_ref();
    [
        ("Session", usage.and_then(|usage| usage.primary.as_ref())),
        ("Weekly", usage.and_then(|usage| usage.secondary.as_ref())),
        ("Monthly", usage.and_then(|usage| usage.tertiary.as_ref())),
    ]
    .into_iter()
    .filter_map(|(label, window)| window.map(|window| (label, window)))
}

fn snapshot_percent(snapshot: &Snapshot) -> f32 {
    available_windows(snapshot)
        .map(|(_, window)| window.used_percent)
        .max_by(f32::total_cmp)
        .unwrap_or_default()
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

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = ();

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = "com.github.leonardo-panseri.cosmic-ext-applet-codexbar";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Construct the app model with the runtime's core.
        let app = AppModel {
            core,
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => {
                        // for why in errors {
                        //     tracing::error!(%why, "error loading app config");
                        // }

                        config
                    }
                })
                .unwrap_or_default(),
            ..Default::default()
        };

        (
            app,
            cosmic::task::future(async {
                Message::SnapshotsLoaded(
                    codexbar::usage_snapshot()
                        .await
                        .map_err(|error| error.to_string()),
                )
            }),
        )
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    /// Describes the interface based on the current state of the application model.
    ///
    /// The applet's button in the panel will be drawn using the main view method.
    /// This view should emit messages to toggle the applet's popup window, which will
    /// be drawn using the `view_window` method.
    fn view(&self) -> Element<'_, Self::Message> {
        let snapshot = self.selected_snapshot();
        let has_error = self.error_detail().is_some();
        let label = match self.config.icon_style {
            IconStyle::Only => None,
            IconStyle::Percent => Some(
                snapshot
                    .map(|snapshot| format!("{:.0}%", snapshot_percent(snapshot)))
                    .unwrap_or_else(|| "…".into()),
            ),
            IconStyle::Label => Some(
                snapshot
                    .map(|snapshot| snapshot.provider.as_str())
                    .filter(|provider| !provider.is_empty())
                    .unwrap_or("CodexBar")
                    .to_owned(),
            ),
        };

        self.panel_button(label, has_error)
    }

    /// The applet's popup window will be drawn using this view method. If there are
    /// multiple poups, you may match the id parameter to determine which popup to
    /// create a view for.
    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let content = if let Some(snapshot) = self.selected_snapshot() {
            self.provider_card(snapshot)
        } else if let Some(error) = self.error_detail() {
            widget::container(
                widget::column::with_capacity(3)
                    .push(widget::text::heading(fl!("refresh-error")))
                    .push(widget::text::body(error))
                    .push(widget::button::text(fl!("copy-error")).on_press(Message::CopyError))
                    .spacing(12),
            )
            .padding(16)
            .width(Length::Fill)
            .into()
        } else {
            widget::container(widget::text::body(fl!("loading-usage")))
                .padding(16)
                .width(Length::Fill)
                .into()
        };

        self.core.applet.popup_container(content).into()
    }

    /// Register subscriptions for this application.
    ///
    /// Subscriptions are long-lived async tasks running in the background which
    /// emit messages to the application through a channel. They may be conditionally
    /// activated by selectively appending to the subscription batch, and will
    /// continue to execute for the duration that they remain in the batch.
    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch(vec![
            // Watch for application configuration changes.
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| {
                    // for why in update.errors {
                    //     tracing::error!(?why, "app config error");
                    // }

                    Message::UpdateConfig(update.config)
                }),
        ])
    }

    /// Handles messages emitted by the application and its widgets.
    ///
    /// Tasks may be returned for asynchronous execution of code in the background
    /// on the application's async runtime. The application will not exit until all
    /// tasks are finished.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::SnapshotsLoaded(result) => self.apply_snapshot_result(result),
            Message::UpdateConfig(config) => {
                self.config = config;
            }
            Message::CopyError => {
                if let Some(error) = self.error_detail() {
                    return clipboard::write(error);
                }
            }
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);
                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );
                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(372.0)
                        .min_width(300.0)
                        .min_height(200.0)
                        .max_height(1080.0);
                    get_popup(popup_settings)
                };
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codexbar::{ProviderError, Snapshot, Usage, Window};

    #[test]
    fn unavailable_windows_are_omitted() {
        let snapshot = Snapshot {
            usage: Some(Usage {
                secondary: Some(Window {
                    used_percent: 42.0,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let windows = available_windows(&snapshot).collect::<Vec<_>>();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].0, "Weekly");
    }

    #[test]
    fn refresh_error_keeps_last_good_snapshot() {
        let mut app = AppModel::default();
        app.apply_snapshot_result(Ok(vec![Snapshot {
            provider: "codex".into(),
            ..Default::default()
        }]));

        app.apply_snapshot_result(Err("network unavailable".into()));

        assert_eq!(app.snapshots[0].provider, "codex");
        assert_eq!(app.refresh_error.as_deref(), Some("network unavailable"));
    }

    #[test]
    fn error_from_another_provider_marks_the_merged_indicator() {
        let mut app = AppModel::default();
        app.snapshots = vec![
            Snapshot {
                provider: "codex".into(),
                usage: Some(Usage::default()),
                ..Default::default()
            },
            Snapshot {
                provider: "openai".into(),
                error: Some(ProviderError {
                    message: "token expired".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        assert_eq!(app.error_detail().as_deref(), Some("openai: token expired"));
    }
}
