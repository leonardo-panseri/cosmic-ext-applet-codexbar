// SPDX-License-Identifier: MPL-2.0

use crate::app::Message;
use crate::codexbar::{ConfigDump, HooksConfig, Provider, Snapshot};
use crate::config::{
    Config, IconStyle, PresentationLayout, RefreshInterval, ResetDisplay, UsageBarFill,
};
use crate::{fl, popup, spend};
use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget::{self, nav_bar, settings, toggler};
use std::path::PathBuf;

const PROJECT_URL: &str = env!("CARGO_PKG_REPOSITORY");
const UPSTREAM_URL: &str = "https://github.com/steipete/CodexBar";

pub fn filtered_providers<'a>(providers: &'a [Provider], query: &str) -> Vec<&'a Provider> {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| !character.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let query = normalize(query);
    providers
        .iter()
        .filter(|provider| {
            normalize(&provider.id).contains(&query)
                || normalize(&provider.display_name).contains(&query)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Pane {
    General,
    UsageSpend,
    MenuBar,
    Menu,
    Notifications,
    Advanced,
    Hooks,
    About,
    Providers,
}

pub struct State {
    pub nav: nav_bar::Model,
    pub search: String,
    pub selected_provider: Option<String>,
    pub dump: ConfigDump,
    pub error: Option<String>,
    pub hooks: HooksConfig,
    pub notification_provider: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        let mut nav = nav_bar::Model::default();
        for (title, pane) in [
            (fl!("settings-general"), Pane::General),
            (fl!("settings-usage-spend"), Pane::UsageSpend),
            (fl!("settings-menu-bar"), Pane::MenuBar),
            (fl!("settings-menu"), Pane::Menu),
            (fl!("settings-notifications"), Pane::Notifications),
            (fl!("settings-advanced"), Pane::Advanced),
            (fl!("settings-hooks"), Pane::Hooks),
            (fl!("settings-about"), Pane::About),
            (fl!("settings-providers"), Pane::Providers),
        ] {
            nav.insert().text(title).data(pane);
        }
        nav.activate_position(0);
        Self {
            nav,
            search: String::new(),
            selected_provider: None,
            dump: ConfigDump::default(),
            error: None,
            hooks: HooksConfig::default(),
            notification_provider: None,
        }
    }
}

impl State {
    pub fn pane(&self) -> Pane {
        self.nav
            .active_data::<Pane>()
            .copied()
            .unwrap_or(Pane::General)
    }

    pub fn select_pane(&mut self, id: nav_bar::Id) {
        self.nav.activate(id);
    }
}

#[derive(Debug, Clone)]
pub enum ConfigChange {
    RefreshInterval(usize),
    TerminalApp(String),
    RefreshOnOpen(bool),
    CheckStatus(bool),
    Language(String),
    LaunchAtLogin(bool),
    IconStyle(usize),
    HighContrast(bool),
    Presentation(usize),
    SwitcherRows(usize),
    UsageBarFill(usize),
    WarningMarkers(bool),
    ResetDisplay(usize),
    ShowCredits(bool),
    ShowCostSummary(bool),
    CostSummaryDays(usize),
    HidePersonalInfo(bool),
    ShowStorageUsage(bool),
    ShowDebug(bool),
    OverviewProvider(String, bool),
    NotificationsEnabled(bool),
    NotificationSound(bool),
    QuotaThreshold(Option<String>, usize, usize),
    ClearProviderThreshold(String),
}

impl ConfigChange {
    pub fn launch_at_login(&self) -> Option<bool> {
        match self {
            Self::LaunchAtLogin(value) => Some(*value),
            _ => None,
        }
    }

    pub fn apply(self, config: &mut Config) {
        match self {
            Self::RefreshInterval(index) => {
                config.refresh_interval = [
                    RefreshInterval::Adaptive,
                    RefreshInterval::OneMinute,
                    RefreshInterval::TwoMinutes,
                    RefreshInterval::FiveMinutes,
                    RefreshInterval::FifteenMinutes,
                    RefreshInterval::ThirtyMinutes,
                ][index.min(5)]
            }
            Self::TerminalApp(value) => config.terminal_app = value,
            Self::RefreshOnOpen(value) => config.refresh_on_popup_open = value,
            Self::CheckStatus(value) => config.check_provider_status = value,
            Self::Language(value) => config.language_override = value,
            Self::LaunchAtLogin(value) => config.launch_at_login = value,
            Self::IconStyle(index) => {
                config.icon_style =
                    [IconStyle::Only, IconStyle::Percent, IconStyle::Label][index.min(2)]
            }
            Self::HighContrast(value) => config.high_contrast = value,
            Self::Presentation(index) => {
                config.presentation_layout = [
                    PresentationLayout::Compact,
                    PresentationLayout::Balanced,
                    PresentationLayout::Detailed,
                ][index.min(2)]
            }
            Self::SwitcherRows(index) => config.switcher_rows = (index + 1).min(6) as u8,
            Self::UsageBarFill(index) => {
                config.usage_bar_fill = [UsageBarFill::Solid, UsageBarFill::Segmented][index.min(1)]
            }
            Self::WarningMarkers(value) => config.warning_markers = value,
            Self::ResetDisplay(index) => {
                config.reset_display = [ResetDisplay::Countdown, ResetDisplay::Clock][index.min(1)]
            }
            Self::ShowCredits(value) => config.show_credits = value,
            Self::ShowCostSummary(value) => config.show_cost_summary = value,
            Self::CostSummaryDays(index) => config.cost_summary_days = [7, 30][index.min(1)],
            Self::HidePersonalInfo(value) => config.hide_personal_info = value,
            Self::ShowStorageUsage(value) => config.show_storage_usage = value,
            Self::ShowDebug(value) => config.show_debug_settings = value,
            Self::OverviewProvider(provider, selected) => {
                config.overview_providers.retain(|item| item != &provider);
                if selected && config.overview_providers.len() < 6 {
                    config.overview_providers.push(provider);
                }
            }
            Self::NotificationsEnabled(value) => config.notifications_enabled = value,
            Self::NotificationSound(value) => config.notification_sound = value,
            Self::QuotaThreshold(provider, lane, index) => {
                let value = [50, 75, 80, 85, 90, 95][index.min(5)];
                let defaults = config.quota_thresholds.defaults;
                let thresholds = match provider {
                    Some(provider) => config
                        .quota_thresholds
                        .providers
                        .entry(provider)
                        .or_insert(defaults),
                    None => &mut config.quota_thresholds.defaults,
                };
                match lane {
                    0 => thresholds.primary = value,
                    1 => thresholds.secondary = value,
                    _ => thresholds.tertiary = value,
                }
            }
            Self::ClearProviderThreshold(provider) => {
                config.quota_thresholds.providers.remove(&provider);
            }
        }
        config.normalize();
    }
}

pub fn sync_autostart(enabled: bool) -> Result<(), String> {
    let config_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "no user config directory is available".to_owned())?;
    let directory = config_dir.join("autostart");
    let path = directory.join(format!("{}.desktop", env!("CARGO_PKG_NAME")));
    if enabled {
        std::fs::create_dir_all(&directory)
            .and_then(|()| std::fs::write(path, autostart_entry()))
            .map_err(|error| format!("failed to enable launch at login: {error}"))
    } else if path.exists() {
        std::fs::remove_file(path)
            .map_err(|error| format!("failed to disable launch at login: {error}"))
    } else {
        Ok(())
    }
}

pub async fn open_link(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only HTTPS links can be opened".into());
    }
    tokio::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map(drop)
        .map_err(|error| format!("failed to open {url}: {error}"))
}

fn autostart_entry() -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=CodexBar\nExec={}\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
        env!("CARGO_PKG_NAME")
    )
}

pub fn view<'a>(
    state: &'a State,
    spend_state: &'a spend::State,
    config: &'a Config,
    providers: &'a [Provider],
    snapshots: &'a [Snapshot],
    refreshing: bool,
    usage_error: Option<String>,
) -> Element<'a, Message> {
    let page = match state.pane() {
        Pane::General => general(config),
        Pane::UsageSpend => spend::view(spend_state),
        Pane::MenuBar => menu_bar(config, providers),
        Pane::Menu => menu(config),
        Pane::Notifications => notifications(state, config, providers),
        Pane::Advanced => advanced(config),
        Pane::Hooks => hooks(state, providers),
        Pane::About => about(),
        Pane::Providers => {
            providers_view(state, config, providers, snapshots, refreshing, usage_error)
        }
    };

    let content = widget::row::with_capacity(2)
        .push(
            widget::nav_bar(&state.nav, Message::SelectSettingsPane)
                .into_container()
                .width(Length::Fixed(220.0)),
        )
        .push(
            widget::container(widget::scrollable(page))
                .padding(24)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .height(Length::Fill);

    widget::column::with_capacity(2)
        .push(
            widget::header_bar()
                .title(fl!("settings-title"))
                .on_close(Message::CloseSettings),
        )
        .push(content)
        .height(Length::Fill)
        .into()
}

fn general(config: &Config) -> Element<'_, Message> {
    settings::view_column(vec![
        widget::text::title3(fl!("settings-general")).into(),
        settings::section()
            .add(settings::item(
                fl!("settings-refresh-interval"),
                widget::dropdown(
                    vec![
                        fl!("refresh-adaptive"),
                        fl!("refresh-one-minute"),
                        fl!("refresh-two-minutes"),
                        fl!("refresh-five-minutes"),
                        fl!("refresh-fifteen-minutes"),
                        fl!("refresh-thirty-minutes"),
                    ],
                    Some(refresh_interval_index(config.refresh_interval)),
                    |index| Message::SettingsConfig(ConfigChange::RefreshInterval(index)),
                ),
            ))
            .add(settings::item(
                fl!("settings-terminal-app"),
                widget::text_input(fl!("settings-terminal-placeholder"), &config.terminal_app)
                    .on_input(|value| Message::SettingsConfig(ConfigChange::TerminalApp(value))),
            ))
            .add(settings::item(
                fl!("settings-language-override"),
                widget::text_input(fl!("settings-system-default"), &config.language_override)
                    .on_input(|value| Message::SettingsConfig(ConfigChange::Language(value))),
            ))
            .add(settings::item(
                fl!("settings-refresh-on-open"),
                toggler(config.refresh_on_popup_open)
                    .on_toggle(|value| Message::SettingsConfig(ConfigChange::RefreshOnOpen(value))),
            ))
            .add(settings::item(
                fl!("settings-check-status"),
                toggler(config.check_provider_status)
                    .on_toggle(|value| Message::SettingsConfig(ConfigChange::CheckStatus(value))),
            ))
            .add(settings::item(
                fl!("settings-launch-at-login"),
                toggler(config.launch_at_login)
                    .on_toggle(|value| Message::SettingsConfig(ConfigChange::LaunchAtLogin(value))),
            ))
            .into(),
        settings::section()
            .add(settings::item_row(vec![
                widget::button::destructive(fl!("settings-quit"))
                    .on_press(Message::Quit)
                    .into(),
            ]))
            .into(),
    ])
    .into()
}

fn menu_bar<'a>(config: &'a Config, providers: &'a [Provider]) -> Element<'a, Message> {
    let mut overview = settings::section().title(fl!("settings-overview-providers"));
    for provider in providers {
        let id = provider.id.clone();
        overview = overview.add(settings::item(
            provider_name(provider),
            toggler(config.overview_providers.contains(&provider.id)).on_toggle(move |value| {
                Message::SettingsConfig(ConfigChange::OverviewProvider(id.clone(), value))
            }),
        ));
    }

    settings::view_column(vec![
        widget::text::title3(fl!("settings-menu-bar")).into(),
        settings::section()
            .add(settings::item(
                fl!("settings-icon-style"),
                widget::dropdown(
                    vec![fl!("icon-only"), fl!("icon-percent"), fl!("icon-label")],
                    Some(icon_style_index(config.icon_style)),
                    |index| Message::SettingsConfig(ConfigChange::IconStyle(index)),
                ),
            ))
            .add(settings::item(
                fl!("settings-high-contrast"),
                toggler(config.high_contrast)
                    .on_toggle(|value| Message::SettingsConfig(ConfigChange::HighContrast(value))),
            ))
            .add(settings::item(
                fl!("settings-layout-preset"),
                widget::dropdown(
                    vec![
                        fl!("layout-compact"),
                        fl!("layout-balanced"),
                        fl!("layout-detailed"),
                    ],
                    Some(presentation_index(config.presentation_layout)),
                    |index| Message::SettingsConfig(ConfigChange::Presentation(index)),
                ),
            ))
            .add(settings::item(
                fl!("settings-switcher-rows"),
                widget::dropdown(
                    vec!["1", "2", "3", "4", "5", "6"],
                    Some(usize::from(config.switcher_rows.saturating_sub(1))),
                    |index| Message::SettingsConfig(ConfigChange::SwitcherRows(index)),
                ),
            ))
            .into(),
        overview.into(),
    ])
    .into()
}

fn menu(config: &Config) -> Element<'_, Message> {
    settings::view_column(vec![
        widget::text::title3(fl!("settings-menu")).into(),
        settings::section()
            .add(settings::item(
                fl!("settings-usage-bar-fill"),
                widget::dropdown(
                    vec![fl!("fill-solid"), fl!("fill-segmented")],
                    Some(usize::from(
                        config.usage_bar_fill == UsageBarFill::Segmented,
                    )),
                    |index| Message::SettingsConfig(ConfigChange::UsageBarFill(index)),
                ),
            ))
            .add(settings::item(
                fl!("settings-warning-markers"),
                toggler(config.warning_markers).on_toggle(|value| {
                    Message::SettingsConfig(ConfigChange::WarningMarkers(value))
                }),
            ))
            .add(settings::item(
                fl!("settings-reset-display"),
                widget::dropdown(
                    vec![fl!("reset-countdown"), fl!("reset-clock")],
                    Some(usize::from(config.reset_display == ResetDisplay::Clock)),
                    |index| Message::SettingsConfig(ConfigChange::ResetDisplay(index)),
                ),
            ))
            .add(settings::item(
                fl!("settings-show-credits"),
                toggler(config.show_credits)
                    .on_toggle(|value| Message::SettingsConfig(ConfigChange::ShowCredits(value))),
            ))
            .add(settings::item(
                fl!("settings-show-cost-summary"),
                toggler(config.show_cost_summary).on_toggle(|value| {
                    Message::SettingsConfig(ConfigChange::ShowCostSummary(value))
                }),
            ))
            .add(settings::item(
                fl!("settings-cost-summary-days"),
                widget::dropdown(
                    vec![fl!("cost-seven-days"), fl!("cost-thirty-days")],
                    Some(usize::from(config.cost_summary_days > 7)),
                    |index| Message::SettingsConfig(ConfigChange::CostSummaryDays(index)),
                ),
            ))
            .into(),
    ])
    .into()
}

fn advanced(config: &Config) -> Element<'_, Message> {
    settings::view_column(vec![
        widget::text::title3(fl!("settings-advanced")).into(),
        settings::section()
            .add(settings::item(
                fl!("settings-hide-personal-info"),
                toggler(config.hide_personal_info).on_toggle(|value| {
                    Message::SettingsConfig(ConfigChange::HidePersonalInfo(value))
                }),
            ))
            .add(settings::item(
                fl!("settings-show-storage"),
                toggler(config.show_storage_usage).on_toggle(|value| {
                    Message::SettingsConfig(ConfigChange::ShowStorageUsage(value))
                }),
            ))
            .add(settings::item(
                fl!("settings-show-debug"),
                toggler(config.show_debug_settings)
                    .on_toggle(|value| Message::SettingsConfig(ConfigChange::ShowDebug(value))),
            ))
            .into(),
    ])
    .into()
}

fn notifications<'a>(
    state: &'a State,
    config: &'a Config,
    providers: &'a [Provider],
) -> Element<'a, Message> {
    let selected = state
        .notification_provider
        .as_deref()
        .and_then(|id| providers.iter().position(|provider| provider.id == id))
        .map_or(0, |index| index + 1);
    let mut provider_names = vec![fl!("settings-global-defaults")];
    provider_names.extend(
        providers
            .iter()
            .map(|provider| provider_name(provider).to_owned()),
    );
    let thresholds = state
        .notification_provider
        .as_deref()
        .map_or(config.quota_thresholds.defaults, |provider| {
            config.quota_thresholds.for_provider(provider)
        });
    let provider = state.notification_provider.clone();
    let mut threshold_section = settings::section()
        .title(fl!("settings-quota-thresholds"))
        .add(settings::item(
            fl!("settings-threshold-provider"),
            widget::dropdown(
                provider_names,
                Some(selected),
                Message::SelectNotificationProvider,
            ),
        ));
    for (lane, label, value) in [
        (0, fl!("settings-session-threshold"), thresholds.primary),
        (1, fl!("settings-weekly-threshold"), thresholds.secondary),
        (2, fl!("settings-monthly-threshold"), thresholds.tertiary),
    ] {
        let target = provider.clone();
        threshold_section = threshold_section.add(settings::item(
            label,
            widget::dropdown(
                vec!["50%", "75%", "80%", "85%", "90%", "95%"],
                Some(threshold_index(value)),
                move |index| {
                    Message::SettingsConfig(ConfigChange::QuotaThreshold(
                        target.clone(),
                        lane,
                        index,
                    ))
                },
            ),
        ));
    }
    if let Some(provider) = &state.notification_provider {
        threshold_section = threshold_section.add(settings::item_row(vec![
            widget::button::text(fl!("settings-use-global-thresholds"))
                .on_press(Message::SettingsConfig(
                    ConfigChange::ClearProviderThreshold(provider.clone()),
                ))
                .into(),
        ]));
    }

    settings::view_column(vec![
        widget::text::title3(fl!("settings-notifications")).into(),
        settings::section()
            .add(settings::item(
                fl!("settings-enable-notifications"),
                toggler(config.notifications_enabled).on_toggle(|value| {
                    Message::SettingsConfig(ConfigChange::NotificationsEnabled(value))
                }),
            ))
            .add(settings::item(
                fl!("settings-notification-sound"),
                toggler(config.notification_sound).on_toggle(|value| {
                    Message::SettingsConfig(ConfigChange::NotificationSound(value))
                }),
            ))
            .into(),
        threshold_section.into(),
    ])
    .into()
}

fn hooks<'a>(state: &'a State, providers: &'a [Provider]) -> Element<'a, Message> {
    let mut rules = settings::section().title(fl!("settings-hook-rules"));
    if state.hooks.events.is_empty() {
        rules = rules.add(settings::item(
            fl!("settings-no-hook-rules"),
            widget::text::body(fl!("settings-hooks-config-file")),
        ));
    }
    for rule in &state.hooks.events {
        let provider = rule
            .provider
            .clone()
            .or_else(|| providers.first().map(|provider| provider.id.clone()))
            .unwrap_or_else(|| "codex".into());
        let detail = format!("{} · {}", rule.event, rule.executable);
        rules = rules.add(settings::item(
            if rule.id.is_empty() {
                &rule.event
            } else {
                &rule.id
            },
            widget::row::with_capacity(2)
                .push(widget::text::caption(detail))
                .push(
                    widget::button::text(fl!("settings-test-hook"))
                        .on_press(Message::TestHook(rule.event.clone(), provider)),
                ),
        ));
    }
    settings::view_column(vec![
        widget::text::title3(fl!("settings-hooks")).into(),
        state
            .error
            .as_ref()
            .map(widget::text::body)
            .unwrap_or_else(|| widget::text::body(""))
            .into(),
        settings::section()
            .add(settings::item(
                fl!("settings-enable-hooks"),
                toggler(state.hooks.enabled).on_toggle(Message::SetHooksEnabled),
            ))
            .into(),
        rules.into(),
    ])
    .into()
}

fn threshold_index(value: u8) -> usize {
    [50, 75, 80, 85, 90, 95]
        .iter()
        .position(|threshold| *threshold == value)
        .unwrap_or(2)
}

fn about() -> Element<'static, Message> {
    settings::view_column(vec![
        widget::text::title3(fl!("settings-about")).into(),
        settings::section()
            .add(settings::item(
                fl!("settings-version"),
                widget::text(env!("CARGO_PKG_VERSION")),
            ))
            .add(settings::item(
                fl!("settings-source"),
                widget::button::text(fl!("settings-project-link"))
                    .on_press(Message::OpenLink(PROJECT_URL.into())),
            ))
            .add(settings::item(
                fl!("settings-upstream"),
                widget::button::text(fl!("settings-upstream-link"))
                    .on_press(Message::OpenLink(UPSTREAM_URL.into())),
            ))
            .into(),
    ])
    .into()
}

fn providers_view<'a>(
    state: &'a State,
    config: &'a Config,
    providers: &'a [Provider],
    snapshots: &'a [Snapshot],
    refreshing: bool,
    usage_error: Option<String>,
) -> Element<'a, Message> {
    let mut list = widget::column::with_capacity(providers.len() + 1)
        .push(
            widget::text_input::search_input(fl!("settings-search-providers"), &state.search)
                .on_input(Message::ProviderSearch),
        )
        .spacing(8);
    for provider in filtered_providers(providers, &state.search) {
        let select_id = provider.id.clone();
        let toggle_id = provider.id.clone();
        let status = snapshots
            .iter()
            .find(|snapshot| snapshot.provider == provider.id)
            .map(|snapshot| {
                if snapshot.error.is_some() {
                    "⊗"
                } else if snapshot.status.as_ref().is_some_and(|status| {
                    !matches!(status.indicator.as_str(), "" | "none" | "operational")
                }) {
                    "◉"
                } else {
                    "●"
                }
            })
            .unwrap_or("○");
        list = list.push(
            settings::item_row(vec![
                widget::text::body(status).into(),
                widget::button::text(provider_name(provider))
                    .on_press(Message::SelectProvider(select_id))
                    .width(Length::Fill)
                    .into(),
                toggler(provider.enabled)
                    .on_toggle(move |enabled| {
                        Message::SetProviderEnabled(toggle_id.clone(), enabled)
                    })
                    .into(),
            ])
            .padding(8),
        );
    }

    let detail: Element<'a, Message> = if let Some(provider_id) = &state.selected_provider {
        provider_detail(
            state,
            config,
            provider_id,
            snapshots,
            refreshing,
            usage_error,
        )
    } else {
        widget::container(widget::text::body(fl!("settings-select-provider")))
            .padding(16)
            .into()
    };

    let page = widget::column::with_capacity(3)
        .push(widget::text::title3(fl!("settings-providers")))
        .push_maybe(state.error.as_ref().map(widget::text::body))
        .push(
            widget::row::with_capacity(2)
                .push(widget::scrollable(list).width(Length::FillPortion(2)))
                .push(widget::scrollable(detail).width(Length::FillPortion(3)))
                .spacing(16)
                .height(Length::Fill),
        )
        .spacing(16);
    widget::container(page).height(Length::Fill).into()
}

fn provider_detail<'a>(
    state: &'a State,
    config: &'a Config,
    provider_id: &'a str,
    snapshots: &'a [Snapshot],
    refreshing: bool,
    usage_error: Option<String>,
) -> Element<'a, Message> {
    let mut content = widget::column::with_capacity(8)
        .push(widget::text::heading(provider_id))
        .spacing(12);
    if crate::codexbar::sign_in_command(provider_id).is_some() {
        content = content.push(
            widget::button::suggested(fl!("settings-sign-in"))
                .on_press(Message::SignIn(provider_id.to_owned())),
        );
    }
    if let Some(provider) = state
        .dump
        .providers
        .iter()
        .find(|provider| provider.id == provider_id)
    {
        let mut fields = settings::section().title(fl!("settings-configuration"));
        for (name, value) in &provider.fields {
            fields = fields.add(settings::item(name, widget::text(display_value(value))));
        }
        content = content.push(fields);
    } else {
        content = content.push(widget::text::body(fl!("settings-no-provider-fields")));
    }
    if let Some(snapshot) = snapshots
        .iter()
        .find(|snapshot| snapshot.provider == provider_id)
    {
        content = content
            .push(widget::text::heading(fl!("settings-live-preview")))
            .push(popup::provider_card(
                snapshot,
                refreshing,
                usage_error,
                config,
            ));
    }
    content.into()
}

fn display_value(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn provider_name(provider: &Provider) -> &str {
    if provider.display_name.is_empty() {
        &provider.id
    } else {
        &provider.display_name
    }
}

fn refresh_interval_index(value: RefreshInterval) -> usize {
    match value {
        RefreshInterval::Adaptive => 0,
        RefreshInterval::OneMinute => 1,
        RefreshInterval::TwoMinutes => 2,
        RefreshInterval::FiveMinutes => 3,
        RefreshInterval::FifteenMinutes => 4,
        RefreshInterval::ThirtyMinutes => 5,
    }
}

fn icon_style_index(value: IconStyle) -> usize {
    match value {
        IconStyle::Only => 0,
        IconStyle::Percent => 1,
        IconStyle::Label => 2,
    }
}

fn presentation_index(value: PresentationLayout) -> usize {
    match value {
        PresentationLayout::Compact => 0,
        PresentationLayout::Balanced => 1,
        PresentationLayout::Detailed => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_search_matches_name_or_identifier_case_insensitively() {
        let providers = vec![
            Provider {
                id: "azureopenai".into(),
                display_name: "Azure OpenAI".into(),
                ..Default::default()
            },
            Provider {
                id: "claude".into(),
                display_name: "Claude".into(),
                ..Default::default()
            },
        ];

        assert_eq!(
            filtered_providers(&providers, "OPEN ai")[0].id,
            "azureopenai"
        );
        assert_eq!(filtered_providers(&providers, "CLAUDE")[0].id, "claude");
        assert!(filtered_providers(&providers, "gemini").is_empty());
    }

    #[test]
    fn autostart_entry_launches_the_applet_without_showing_a_menu_entry() {
        let entry = autostart_entry();
        assert!(entry.contains("Exec=cosmic-ext-applet-codexbar"));
        assert!(entry.contains("NoDisplay=true"));
    }

    #[test]
    fn presentation_controls_update_the_live_card_config() {
        let mut config = Config::default();
        for change in [
            ConfigChange::HighContrast(true),
            ConfigChange::Presentation(2),
            ConfigChange::SwitcherRows(1),
            ConfigChange::UsageBarFill(1),
            ConfigChange::ResetDisplay(1),
            ConfigChange::ShowCredits(false),
            ConfigChange::NotificationsEnabled(false),
            ConfigChange::NotificationSound(false),
            ConfigChange::QuotaThreshold(None, 0, 4),
            ConfigChange::QuotaThreshold(Some("codex".into()), 1, 5),
        ] {
            change.apply(&mut config);
        }

        assert!(config.high_contrast);
        assert_eq!(config.presentation_layout, PresentationLayout::Detailed);
        assert_eq!(config.switcher_rows, 2);
        assert_eq!(config.usage_bar_fill, UsageBarFill::Segmented);
        assert_eq!(config.reset_display, ResetDisplay::Clock);
        assert!(!config.show_credits);
        assert!(!config.notifications_enabled);
        assert!(!config.notification_sound);
        assert_eq!(config.quota_thresholds.defaults.primary, 90);
        assert_eq!(config.quota_thresholds.for_provider("codex").secondary, 95);
    }
}
