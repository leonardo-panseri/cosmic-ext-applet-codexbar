// SPDX-License-Identifier: MPL-2.0

use crate::alerts::{self, Coalescer};
use crate::codexbar::{self, ConfigDump, HooksConfig, Provider, Snapshot, Window};
use crate::config::{Config, IconStyle, RefreshInterval};
use crate::fl;
use crate::popup;
use crate::settings::{self, ConfigChange};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::{
    Alignment, Length, Limits, Size, Subscription,
    alignment::{Horizontal, Vertical},
    clipboard,
    widget::Stack,
    window::{self, Id},
};
use cosmic::prelude::*;
use cosmic::widget;
use cosmic::widget::segmented_button::{self, SingleSelectModel};
use jiff::Timestamp;
use std::time::Duration;

#[derive(Debug, Default, Clone, Eq, PartialEq)]
enum Selection {
    #[default]
    Auto,
    Overview,
    Provider(String),
}

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
#[derive(Default)]
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// The popup id.
    popup: Option<Id>,
    /// The normal settings window id, when open.
    settings_window: Option<Id>,
    /// Settings navigation and provider-management state.
    settings: settings::State,
    /// Configuration data that persists between application runs.
    config: Config,
    /// Last successfully loaded all-provider snapshot.
    snapshots: Vec<Snapshot>,
    /// Provider metadata in the order returned by codexbar configuration.
    providers: Vec<Provider>,
    /// Provider or overview currently shown in the popup.
    selection: Selection,
    /// Native switcher model rebuilt when snapshots or configuration change.
    provider_tabs: SingleSelectModel,
    /// The latest refresh failure; snapshots remain visible while this is set.
    refresh_error: Option<String>,
    /// Whether a usage request is currently running.
    refreshing: bool,
    /// Stable period used by the refresh subscription.
    refresh_period: Duration,
    /// Baseline and coalescing state for edge-triggered alerts and hooks.
    alert_state: Option<alerts::State>,
    hook_coalescer: Coalescer,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    RefreshRequested,
    SnapshotsLoaded(Result<Vec<Snapshot>, String>),
    ProvidersLoaded(Result<Vec<Provider>, String>),
    SelectTab(segmented_button::Entity),
    UpdateConfig(Config),
    CopyError,
    OpenSettings,
    CloseSettings,
    SettingsClosed(Id),
    SettingsOpened,
    SelectSettingsPane(cosmic::widget::nav_bar::Id),
    SelectNotificationProvider(usize),
    ProviderSearch(String),
    SelectProvider(String),
    SetProviderEnabled(String, bool),
    ProviderEnabled(Result<(String, bool), String>),
    ConfigDumpLoaded(Result<ConfigDump, String>),
    HooksLoaded(Result<HooksConfig, String>),
    SetHooksEnabled(bool),
    HooksEnabled(Result<(), String>),
    TestHook(String, String),
    HookTested(Result<(), String>),
    AlertsDelivered(Vec<String>),
    SignIn(String),
    SignInFinished(Result<(), String>),
    SettingsConfig(ConfigChange),
    OpenLink(String),
    LinkOpened(Result<(), String>),
    Quit,
}

impl AppModel {
    fn refresh_task(check_status: bool) -> Task<cosmic::Action<Message>> {
        cosmic::task::future(async move {
            Message::SnapshotsLoaded(
                codexbar::usage_snapshot(check_status)
                    .await
                    .map_err(|error| error.to_string()),
            )
        })
    }

    fn providers_task() -> Task<cosmic::Action<Message>> {
        cosmic::task::future(async {
            Message::ProvidersLoaded(
                codexbar::providers()
                    .await
                    .map_err(|error| error.to_string()),
            )
        })
    }

    fn config_dump_task() -> Task<cosmic::Action<Message>> {
        cosmic::task::future(async {
            Message::ConfigDumpLoaded(
                codexbar::config_dump()
                    .await
                    .map_err(|error| error.to_string()),
            )
        })
    }

    fn hooks_task() -> Task<cosmic::Action<Message>> {
        cosmic::task::future(async {
            Message::HooksLoaded(codexbar::hooks().await.map_err(|error| error.to_string()))
        })
    }

    fn hooks_toggle_task(enabled: bool) -> Task<cosmic::Action<Message>> {
        cosmic::task::future(async move {
            Message::HooksEnabled(
                codexbar::set_hooks_enabled(enabled)
                    .await
                    .map_err(|error| error.to_string()),
            )
        })
    }

    fn hook_test_task(event: String, provider: String) -> Task<cosmic::Action<Message>> {
        cosmic::task::future(async move {
            Message::HookTested(
                codexbar::test_hook(&event, &provider)
                    .await
                    .map_err(|error| error.to_string()),
            )
        })
    }

    fn alerts_task(
        notifications: Vec<alerts::Event>,
        hook_events: Vec<alerts::Event>,
        hooks: HooksConfig,
        sound: bool,
        hide_personal_info: bool,
    ) -> Task<cosmic::Action<Message>> {
        cosmic::task::future(async move {
            Message::AlertsDelivered(
                alerts::deliver(notifications, hook_events, hooks, sound, hide_personal_info).await,
            )
        })
    }

    fn provider_toggle_task(provider: String, enabled: bool) -> Task<cosmic::Action<Message>> {
        cosmic::task::future(async move {
            Message::ProviderEnabled(
                codexbar::set_provider_enabled(&provider, enabled)
                    .await
                    .map(|()| (provider, enabled))
                    .map_err(|error| error.to_string()),
            )
        })
    }

    fn sign_in_task(provider: String) -> Task<cosmic::Action<Message>> {
        cosmic::task::future(async move {
            Message::SignInFinished(
                codexbar::launch_sign_in(&provider)
                    .await
                    .map_err(|error| error.to_string()),
            )
        })
    }

    fn open_link_task(url: String) -> Task<cosmic::Action<Message>> {
        cosmic::task::future(async move { Message::LinkOpened(settings::open_link(url).await) })
    }

    fn begin_refresh(&mut self) -> bool {
        if self.refreshing {
            return false;
        }
        self.refreshing = true;
        true
    }

    fn apply_snapshot_result(
        &mut self,
        result: Result<Vec<Snapshot>, String>,
    ) -> Vec<alerts::Event> {
        self.refreshing = false;
        let current = match result {
            Ok(mut snapshots) => {
                for snapshot in snapshots
                    .iter_mut()
                    .filter(|snapshot| snapshot.usage.is_none())
                {
                    let Some(error) = snapshot.error.take() else {
                        continue;
                    };
                    if let Some(previous) = self
                        .snapshots
                        .iter()
                        .find(|previous| previous.provider == snapshot.provider)
                    {
                        *snapshot = previous.clone();
                    }
                    snapshot.error = Some(error);
                }
                self.snapshots = snapshots;
                if matches!(self.selection, Selection::Auto)
                    || matches!(
                        &self.selection,
                        Selection::Provider(provider)
                            if !self.snapshots.iter().any(|snapshot| &snapshot.provider == provider)
                    )
                {
                    self.selection = most_used_provider(&self.snapshots).unwrap_or_default();
                }
                self.refresh_error = None;
                self.refresh_period = refresh_period(
                    self.config.refresh_interval,
                    &self.snapshots,
                    Timestamp::now(),
                );
                self.rebuild_provider_tabs();
                alerts::State::loaded(self.snapshots.clone())
            }
            Err(error) => {
                self.refresh_error = Some(error.clone());
                alerts::State::failed(error)
            }
        };
        let events = alerts::diff(
            self.alert_state.as_ref(),
            &current,
            &self.config.quota_thresholds,
        );
        self.alert_state = Some(current);
        events
    }

    fn selected_snapshot(&self) -> Option<&Snapshot> {
        let Selection::Provider(provider) = &self.selection else {
            return self
                .snapshots
                .iter()
                .filter(|snapshot| snapshot.usage.is_some())
                .max_by(|left, right| snapshot_percent(left).total_cmp(&snapshot_percent(right)))
                .or_else(|| self.snapshots.first());
        };
        self.snapshots
            .iter()
            .find(|snapshot| &snapshot.provider == provider)
            .or_else(|| self.snapshots.first())
    }

    fn rebuild_provider_tabs(&mut self) {
        self.provider_tabs = provider_tabs(
            &self.snapshots,
            &self.providers,
            &self.config.overview_providers,
            self.config.switcher_rows,
            &self.selection,
        );
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

    fn panel_button(&self, label: Option<String>, has_error: bool) -> Element<'_, Message> {
        let (icon_width, icon_height) = self.core.applet.suggested_size(true);
        let icon = indicator_icon(has_error, icon_width);
        if label.is_none() && !self.refreshing {
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
        let value: Element<'_, Message> = if self.refreshing {
            widget::indeterminate_circular().size(16.0).into()
        } else {
            widget::text(label.unwrap_or_default()).into()
        };
        let content = widget::row::with_capacity(2)
            .push(icon)
            .push(value)
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

pub(crate) fn available_windows(
    snapshot: &Snapshot,
) -> impl Iterator<Item = (&'static str, &Window)> {
    let usage = snapshot.usage.as_ref();
    [
        ("Session", usage.and_then(|usage| usage.primary.as_ref())),
        ("Weekly", usage.and_then(|usage| usage.secondary.as_ref())),
        ("Monthly", usage.and_then(|usage| usage.tertiary.as_ref())),
    ]
    .into_iter()
    .filter_map(|(label, window)| window.map(|window| (label, window)))
}

pub(crate) fn snapshot_percent(snapshot: &Snapshot) -> f32 {
    available_windows(snapshot)
        .map(|(_, window)| window.used_percent)
        .max_by(f32::total_cmp)
        .unwrap_or_default()
}

fn most_used_provider(snapshots: &[Snapshot]) -> Option<Selection> {
    snapshots
        .iter()
        .filter(|snapshot| snapshot.usage.is_some())
        .max_by(|left, right| snapshot_percent(left).total_cmp(&snapshot_percent(right)))
        .map(|snapshot| Selection::Provider(snapshot.provider.clone()))
}

fn ordered_snapshots<'a>(snapshots: &'a [Snapshot], providers: &[Provider]) -> Vec<&'a Snapshot> {
    let mut ordered = snapshots.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|snapshot| {
        providers
            .iter()
            .position(|provider| provider.id == snapshot.provider)
            .unwrap_or(usize::MAX)
    });
    ordered
}

fn overview_snapshots<'a>(
    snapshots: &'a [Snapshot],
    providers: &[Provider],
    selected: &[String],
) -> Vec<&'a Snapshot> {
    ordered_snapshots(snapshots, providers)
        .into_iter()
        .filter(|snapshot| {
            selected
                .iter()
                .take(6)
                .any(|provider| provider == &snapshot.provider)
        })
        .take(6)
        .collect()
}

fn provider_tabs(
    snapshots: &[Snapshot],
    providers: &[Provider],
    overview_providers: &[String],
    switcher_rows: u8,
    selection: &Selection,
) -> SingleSelectModel {
    let mut tabs = SingleSelectModel::default();
    if !overview_providers.is_empty() {
        let id = tabs
            .insert()
            .text(fl!("overview"))
            .data(Selection::Overview)
            .id();
        if *selection == Selection::Overview {
            tabs.activate(id);
        }
    }
    for snapshot in ordered_snapshots(snapshots, providers)
        .into_iter()
        .take(usize::from(switcher_rows))
    {
        let tab = Selection::Provider(snapshot.provider.clone());
        let label = providers
            .iter()
            .find(|provider| provider.id == snapshot.provider)
            .map(|provider| provider.display_name.as_str())
            .filter(|label| !label.is_empty())
            .unwrap_or(&snapshot.provider)
            .to_owned();
        let id = tabs.insert().text(label).data(tab.clone()).id();
        if selection == &tab {
            tabs.activate(id);
        }
    }
    tabs
}

fn refresh_period(interval: RefreshInterval, snapshots: &[Snapshot], now: Timestamp) -> Duration {
    if let Some(minutes) = interval.fixed_minutes() {
        return Duration::from_secs(minutes * 60);
    }

    let seconds = snapshots
        .iter()
        .flat_map(available_windows)
        .filter_map(|(_, window)| window.resets_at.as_deref())
        .filter_map(|reset| reset.parse::<Timestamp>().ok())
        .map(|reset| now.duration_until(reset).as_secs())
        .filter(|seconds| *seconds > 0)
        .min()
        .unwrap_or(300)
        .max(60);
    Duration::from_secs(seconds as u64)
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
        let mut app = AppModel {
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
            refreshing: true,
            ..Default::default()
        };
        app.config.normalize();
        app.refresh_period = refresh_period(
            app.config.refresh_interval,
            &app.snapshots,
            Timestamp::now(),
        );

        let check_status = app.config.check_provider_status;
        (
            app,
            Task::batch([
                Self::refresh_task(check_status),
                Self::providers_task(),
                Self::config_dump_task(),
                Self::hooks_task(),
            ]),
        )
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(if self.settings_window == Some(id) {
            Message::SettingsClosed(id)
        } else {
            Message::PopupClosed(id)
        })
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
    fn view_window(&self, id: Id) -> Element<'_, Self::Message> {
        if self.settings_window == Some(id) {
            return settings::view(
                &self.settings,
                &self.config,
                &self.providers,
                &self.snapshots,
                self.refreshing,
                self.error_detail(),
            );
        }

        let body = if self.selection == Selection::Overview {
            popup::overview(
                overview_snapshots(
                    &self.snapshots,
                    &self.providers,
                    &self.config.overview_providers,
                ),
                &self.providers,
            )
        } else if let Some(snapshot) = self.selected_snapshot() {
            crate::popup::provider_card(
                snapshot,
                self.refreshing,
                self.error_detail(),
                &self.config,
            )
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

        let body = if self.provider_tabs.iter().count() > 1 {
            widget::column::with_capacity(2)
                .push(
                    widget::segmented_control::horizontal(&self.provider_tabs)
                        .on_activate(Message::SelectTab),
                )
                .push(body)
                .spacing(12)
                .padding([12, 0])
                .into()
        } else {
            body
        };

        let content = widget::column::with_capacity(3)
            .push(body)
            .push(widget::divider::horizontal::light())
            .push(widget::button::text(fl!("settings")).on_press(Message::OpenSettings));

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
            cosmic::iced::time::every(self.refresh_period).map(|_| Message::RefreshRequested),
        ])
    }

    /// Handles messages emitted by the application and its widgets.
    ///
    /// Tasks may be returned for asynchronous execution of code in the background
    /// on the application's async runtime. The application will not exit until all
    /// tasks are finished.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::RefreshRequested => {
                if self.begin_refresh() {
                    return Self::refresh_task(self.config.check_provider_status);
                }
            }
            Message::SnapshotsLoaded(result) => {
                let events = self.apply_snapshot_result(result);
                if !events.is_empty() {
                    let notifications = if self.config.notifications_enabled {
                        events.clone()
                    } else {
                        Vec::new()
                    };
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default();
                    let hook_events = if self.settings.hooks.enabled {
                        events
                            .into_iter()
                            .filter(|event| self.hook_coalescer.allow(event, now))
                            .collect()
                    } else {
                        Vec::new()
                    };
                    if !notifications.is_empty() || !hook_events.is_empty() {
                        return Self::alerts_task(
                            notifications,
                            hook_events,
                            self.settings.hooks.clone(),
                            self.config.notification_sound,
                            self.config.hide_personal_info,
                        );
                    }
                }
            }
            Message::ProvidersLoaded(Ok(providers)) => {
                self.providers = providers;
                self.rebuild_provider_tabs();
            }
            Message::ProvidersLoaded(Err(_)) => {}
            Message::SelectTab(id) => {
                let Some(selection) = self.provider_tabs.data::<Selection>(id).cloned() else {
                    return Task::none();
                };
                self.selection = selection;
                self.provider_tabs.activate(id);
            }
            Message::UpdateConfig(mut config) => {
                config.normalize();
                self.config = config;
                if self.config.overview_providers.is_empty()
                    && self.selection == Selection::Overview
                {
                    self.selection = most_used_provider(&self.snapshots).unwrap_or_default();
                }
                self.rebuild_provider_tabs();
                self.refresh_period = refresh_period(
                    self.config.refresh_interval,
                    &self.snapshots,
                    Timestamp::now(),
                );
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
                    let popup = get_popup(popup_settings);
                    if self.config.refresh_on_popup_open && self.begin_refresh() {
                        Task::batch([popup, Self::refresh_task(self.config.check_provider_status)])
                    } else {
                        popup
                    }
                };
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
            Message::OpenSettings => {
                if let Some(id) = self.settings_window {
                    return window::gain_focus(id);
                }
                let (id, open) = window::open(window::Settings {
                    size: Size::new(960.0, 720.0),
                    min_size: Some(Size::new(720.0, 520.0)),
                    decorations: false,
                    exit_on_close_request: false,
                    ..Default::default()
                });
                self.settings_window = Some(id);
                _ = self.set_window_title(fl!("settings-title"), id);
                let open = open.map(|_| cosmic::Action::App(Message::SettingsOpened));
                return if let Some(popup) = self.popup.take() {
                    Task::batch([destroy_popup(popup), open])
                } else {
                    open
                };
            }
            Message::CloseSettings => {
                if let Some(id) = self.settings_window.take() {
                    return window::close(id);
                }
            }
            Message::SettingsClosed(id) => {
                if self.settings_window == Some(id) {
                    self.settings_window = None;
                }
                return window::close(id);
            }
            Message::SettingsOpened => {}
            Message::SelectSettingsPane(id) => self.settings.select_pane(id),
            Message::SelectNotificationProvider(index) => {
                self.settings.notification_provider = index
                    .checked_sub(1)
                    .and_then(|index| self.providers.get(index))
                    .map(|provider| provider.id.clone());
            }
            Message::ProviderSearch(query) => self.settings.search = query,
            Message::SelectProvider(provider) => {
                self.settings.selected_provider = Some(provider);
                self.settings.error = None;
            }
            Message::SetProviderEnabled(provider, enabled) => {
                self.settings.error = None;
                return Self::provider_toggle_task(provider, enabled);
            }
            Message::ProviderEnabled(Ok((provider, enabled))) => {
                if let Some(entry) = self.providers.iter_mut().find(|entry| entry.id == provider) {
                    entry.enabled = enabled;
                }
                let mut tasks = vec![Self::config_dump_task()];
                if self.begin_refresh() {
                    tasks.push(Self::refresh_task(self.config.check_provider_status));
                }
                return Task::batch(tasks);
            }
            Message::ProviderEnabled(Err(error)) => self.settings.error = Some(error),
            Message::ConfigDumpLoaded(Ok(dump)) => self.settings.dump = dump,
            Message::ConfigDumpLoaded(Err(error)) => self.settings.error = Some(error),
            Message::HooksLoaded(Ok(hooks)) => self.settings.hooks = hooks,
            Message::HooksLoaded(Err(error)) => self.settings.error = Some(error),
            Message::SetHooksEnabled(enabled) => {
                self.settings.error = None;
                return Self::hooks_toggle_task(enabled);
            }
            Message::HooksEnabled(Ok(())) => return Self::hooks_task(),
            Message::HooksEnabled(Err(error)) => self.settings.error = Some(error),
            Message::TestHook(event, provider) => {
                self.settings.error = None;
                return Self::hook_test_task(event, provider);
            }
            Message::HookTested(Ok(())) => {}
            Message::HookTested(Err(error)) => self.settings.error = Some(error),
            Message::AlertsDelivered(errors) => {
                if let Some(error) = errors.into_iter().next() {
                    self.settings.error = Some(error);
                }
            }
            Message::SignIn(provider) => {
                self.settings.error = None;
                return Self::sign_in_task(provider);
            }
            Message::SignInFinished(Ok(())) => {}
            Message::SignInFinished(Err(error)) => self.settings.error = Some(error),
            Message::SettingsConfig(change) => {
                let launch_at_login = change.launch_at_login();
                if let Some(enabled) = launch_at_login
                    && let Err(error) = settings::sync_autostart(enabled)
                {
                    self.settings.error = Some(error);
                    return Task::none();
                }
                change.apply(&mut self.config);
                match cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                    .and_then(|context| self.config.write_entry(&context))
                {
                    Ok(()) => {}
                    Err(error) => {
                        self.settings.error = Some(format!("failed to save settings: {error}"))
                    }
                }
                self.rebuild_provider_tabs();
                self.refresh_period = refresh_period(
                    self.config.refresh_interval,
                    &self.snapshots,
                    Timestamp::now(),
                );
            }
            Message::OpenLink(url) => return Self::open_link_task(url),
            Message::LinkOpened(Ok(())) => {}
            Message::LinkOpened(Err(error)) => self.settings.error = Some(error),
            Message::Quit => return cosmic::iced::exit(),
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
    use crate::codexbar::{
        HookRule, HooksConfig, Provider, ProviderError, Snapshot, Usage, Window,
    };
    use crate::config::RefreshInterval;
    use jiff::Timestamp;

    #[test]
    fn provider_tabs_follow_configuration_order() {
        let snapshots = vec![
            Snapshot {
                provider: "codex".into(),
                ..Default::default()
            },
            Snapshot {
                provider: "claude".into(),
                ..Default::default()
            },
        ];
        let providers = vec![
            Provider {
                id: "claude".into(),
                ..Default::default()
            },
            Provider {
                id: "codex".into(),
                ..Default::default()
            },
        ];

        let tabs = provider_tabs(
            &snapshots,
            &providers,
            &[],
            6,
            &Selection::Provider("claude".into()),
        );
        assert_eq!(
            tabs.iter()
                .filter_map(|id| tabs.text(id))
                .collect::<Vec<_>>(),
            ["claude", "codex"]
        );
    }

    #[test]
    fn overview_contains_at_most_six_selected_providers() {
        let snapshots = (0..8)
            .map(|index| Snapshot {
                provider: format!("provider-{index}"),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let selected = snapshots
            .iter()
            .map(|snapshot| snapshot.provider.clone())
            .collect::<Vec<_>>();

        assert_eq!(overview_snapshots(&snapshots, &[], &selected).len(), 6);
    }

    #[test]
    fn overview_tab_is_hidden_without_selected_providers() {
        let tabs = provider_tabs(
            &[Snapshot {
                provider: "codex".into(),
                ..Default::default()
            }],
            &[],
            &[],
            6,
            &Selection::Provider("codex".into()),
        );

        assert_eq!(
            tabs.iter()
                .filter_map(|id| tabs.text(id))
                .collect::<Vec<_>>(),
            ["codex"]
        );
    }

    #[test]
    fn switcher_row_count_caps_visible_provider_tabs() {
        let snapshots = (0..4)
            .map(|index| Snapshot {
                provider: format!("provider-{index}"),
                ..Default::default()
            })
            .collect::<Vec<_>>();

        let tabs = provider_tabs(&snapshots, &[], &[], 2, &Selection::default());

        assert_eq!(tabs.iter().count(), 2);
    }

    #[test]
    fn initial_selection_uses_highest_usage() {
        let snapshots = vec![
            Snapshot {
                provider: "codex".into(),
                usage: Some(Usage {
                    secondary: Some(Window {
                        used_percent: 20.0,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            Snapshot {
                provider: "claude".into(),
                usage: Some(Usage {
                    primary: Some(Window {
                        used_percent: 80.0,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let mut app = AppModel::default();
        _ = app.apply_snapshot_result(Ok(snapshots));

        assert_eq!(
            app.selected_snapshot()
                .map(|snapshot| snapshot.provider.as_str()),
            Some("claude")
        );
    }

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
        _ = app.apply_snapshot_result(Ok(vec![Snapshot {
            provider: "codex".into(),
            ..Default::default()
        }]));
        assert!(app.begin_refresh());

        _ = app.apply_snapshot_result(Err("codexbar timed out after 30 seconds".into()));

        assert_eq!(app.snapshots[0].provider, "codex");
        assert_eq!(
            app.refresh_error.as_deref(),
            Some("codexbar timed out after 30 seconds")
        );
        assert!(!app.refreshing);
    }

    #[test]
    fn concurrent_refresh_is_ignored() {
        let mut app = AppModel::default();

        assert!(app.begin_refresh());
        assert!(!app.begin_refresh());
    }

    #[test]
    fn adaptive_refresh_has_a_one_minute_floor() {
        let snapshot = Snapshot {
            usage: Some(Usage {
                primary: Some(Window {
                    resets_at: Some("2026-08-09T10:00:30Z".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert_eq!(
            refresh_period(
                RefreshInterval::Adaptive,
                &[snapshot],
                "2026-08-09T10:00:00Z".parse::<Timestamp>().unwrap()
            ),
            std::time::Duration::from_secs(60)
        );
    }

    #[test]
    fn adaptive_refresh_uses_the_nearest_reset() {
        let snapshot = Snapshot {
            usage: Some(Usage {
                primary: Some(Window {
                    resets_at: Some("2026-08-09T12:00:00Z".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert_eq!(
            refresh_period(
                RefreshInterval::Adaptive,
                &[snapshot],
                "2026-08-09T10:00:00Z".parse::<Timestamp>().unwrap()
            ),
            std::time::Duration::from_secs(2 * 60 * 60)
        );
    }

    #[test]
    fn fixed_refresh_uses_configured_interval() {
        assert_eq!(
            refresh_period(RefreshInterval::FifteenMinutes, &[], Timestamp::UNIX_EPOCH),
            std::time::Duration::from_secs(15 * 60)
        );
    }

    #[test]
    fn successful_refresh_replaces_snapshot_and_settles_loading() {
        let mut app = AppModel {
            snapshots: vec![Snapshot {
                provider: "old".into(),
                ..Default::default()
            }],
            refresh_error: Some("old failure".into()),
            ..Default::default()
        };
        assert!(app.begin_refresh());

        _ = app.apply_snapshot_result(Ok(vec![Snapshot {
            provider: "new".into(),
            ..Default::default()
        }]));

        assert_eq!(app.snapshots[0].provider, "new");
        assert!(app.refresh_error.is_none());
        assert!(!app.refreshing);
    }

    #[test]
    fn provider_failure_keeps_its_last_good_usage() {
        let mut app = AppModel {
            snapshots: vec![Snapshot {
                provider: "codex".into(),
                usage: Some(Usage::default()),
                ..Default::default()
            }],
            ..Default::default()
        };

        _ = app.apply_snapshot_result(Ok(vec![Snapshot {
            provider: "codex".into(),
            error: Some(ProviderError {
                message: "network unavailable".into(),
                ..Default::default()
            }),
            ..Default::default()
        }]));

        assert!(app.snapshots[0].usage.is_some());
        assert_eq!(
            app.snapshots[0]
                .error
                .as_ref()
                .map(|error| error.message.as_str()),
            Some("network unavailable")
        );
    }

    #[test]
    fn error_from_another_provider_marks_the_merged_indicator() {
        let app = AppModel {
            snapshots: vec![
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
            ],
            ..Default::default()
        };

        assert_eq!(app.error_detail().as_deref(), Some("openai: token expired"));
    }

    #[test]
    fn refresh_handler_diffs_previous_and_current_snapshots() {
        let snapshot = |used_percent| Snapshot {
            provider: "codex".into(),
            usage: Some(Usage {
                primary: Some(Window {
                    used_percent,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut app = AppModel::default();
        assert!(
            app.apply_snapshot_result(Ok(vec![snapshot(79.0)]))
                .is_empty()
        );

        let events = app.apply_snapshot_result(Ok(vec![snapshot(80.0)]));

        assert_eq!(events[0].kind, alerts::EventKind::QuotaLow);
    }

    #[test]
    fn hook_load_failure_preserves_last_known_rules() {
        let mut app = AppModel::default();
        let hooks = HooksConfig {
            enabled: true,
            events: vec![HookRule {
                id: "warn".into(),
                ..Default::default()
            }],
        };
        drop(cosmic::Application::update(
            &mut app,
            Message::HooksLoaded(Ok(hooks)),
        ));

        drop(cosmic::Application::update(
            &mut app,
            Message::HooksLoaded(Err("codexbar unavailable".into())),
        ));

        assert!(app.settings.hooks.enabled);
        assert_eq!(app.settings.hooks.events[0].id, "warn");
        assert_eq!(app.settings.error.as_deref(), Some("codexbar unavailable"));
    }
}
