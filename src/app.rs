// SPDX-License-Identifier: MPL-2.0

//! The panel button, the poll loop and what the applet knows about the backend
//! (`docs/implementation-plan.md` D7, §4.1 and §4.2). The `Connected` popup is in [`crate::popup`],
//! the other states' popups and the banners in [`crate::guidance`].

use std::rc::Rc;
use std::time::{Duration, Instant};

use cosmic::iced::platform_specific::shell::wayland::commands::popup::destroy_popup;
use cosmic::iced::widget::{stack, svg};
use cosmic::iced::{Alignment, Border, Color, Length, Limits, Subscription, keyboard, window::Id};
use cosmic::prelude::*;
use cosmic::surface::action::LiveSettings;
use cosmic::widget::{self, icon, segmented_button};

use crate::backend::{self, Backend, BackendError};
use crate::config::{self, Config, PanelStyle};
use crate::guidance::{self, Autostart, TrayPrompt};
use crate::snapshot::{Bar, Meter, Snapshot};
use crate::{icons, popup};

/// At login the applet and the desktop app start together, so failures in this window keep
/// the applet `Connecting` instead of flashing a warning.
const GRACE: Duration = Duration::from_secs(10);
/// Omarchy's cadence. A failed connect costs microseconds, so `Unavailable` polls at it too.
const POLL: Duration = Duration::from_secs(5);
/// While a refresh the applet asked for is running.
const FAST_POLL: Duration = Duration::from_secs(1);
/// The desktop app's own deadline for a refresh.
const FAST_POLL_CAP: Duration = Duration::from_secs(60);

/// What the applet knows about the backend (plan D7).
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Since startup, for up to [`GRACE`], without a usable snapshot.
    Connecting,
    Unavailable(Unavailable),
    /// The backend replied, but in a shape or schema version this applet doesn't understand.
    Incompatible,
    Connected(Snapshot),
}

/// Why the backend can't be reached. It only changes the wording (plan D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// There's no socket: the desktop app isn't installed or isn't running.
    NotRunning,
    /// The socket is there but refuses connections or doesn't answer in time, as when the
    /// desktop app crashed or hangs.
    NotResponding,
}

impl State {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }

    /// The state after a poll that finished `waited` after startup.
    fn after(self, result: Result<Snapshot, BackendError>, waited: Duration) -> Self {
        let unavailable = match result {
            Ok(snapshot) => return Self::Connected(snapshot),
            Err(BackendError::NotRunning) => Unavailable::NotRunning,
            Err(BackendError::NotResponding | BackendError::Timeout) => Unavailable::NotResponding,
            // A reply came back, just not one we can use: no reason to wait for the grace period.
            Err(
                BackendError::Incompatible(_) | BackendError::TooLarge | BackendError::Rejected(_),
            ) => return Self::Incompatible,
        };
        if matches!(self, Self::Connecting) && waited < GRACE {
            Self::Connecting
        } else {
            Self::Unavailable(unavailable)
        }
    }
}

/// The D7 state plus the poll schedule (plan §4.1), kept apart from the GUI so it can be tested
/// with made-up times. Each method that returns `true` means "send a snapshot request now".
#[derive(Debug)]
pub struct Connection {
    started: Instant,
    state: State,
    /// A snapshot request is running. Ticks that find one are skipped.
    in_flight: bool,
    /// A refresh finished, or the popup opened, while a request was running, so that request's
    /// reply may predate the refresh or lack the spending request. Poll again as soon as it returns.
    poll_after: bool,
    /// The popup opened: the next request asks for spending (plan §4.1).
    spending: bool,
    /// After a refresh, poll every [`FAST_POLL`] while the snapshot is `busy`, until this time.
    fast_until: Option<Instant>,
}

impl Connection {
    pub fn new(now: Instant) -> Self {
        Self {
            started: now,
            state: State::Connecting,
            in_flight: false,
            poll_after: false,
            spending: false,
            fast_until: None,
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// How often the subscription should tick.
    pub fn interval(&self, now: Instant) -> Duration {
        if self.fast_until.is_some_and(|until| now < until) {
            FAST_POLL
        } else {
            POLL
        }
    }

    /// A tick of the poll timer, or startup.
    pub fn tick(&mut self) -> bool {
        !std::mem::replace(&mut self.in_flight, true)
    }

    /// The popup opened. Poll with spending now, or as soon as the poll in flight returns.
    pub fn popup_opened(&mut self) -> bool {
        self.spending = true;
        self.poll_soon()
    }

    /// **Check again** in the `Unavailable` popup.
    pub fn check_again(&mut self) -> bool {
        self.poll_soon()
    }

    /// Whether the request about to be sent asks for spending. Call once for each request.
    pub fn take_spending(&mut self) -> bool {
        std::mem::take(&mut self.spending)
    }

    /// The `refresh` request finished. Poll right away, and if the desktop app accepted it, keep
    /// polling fast while it's busy.
    pub fn refreshed(&mut self, now: Instant, accepted: bool) -> bool {
        if accepted {
            self.fast_until = Some(now + FAST_POLL_CAP);
        }
        self.poll_soon()
    }

    /// Poll now, or right after the poll in flight.
    fn poll_soon(&mut self) -> bool {
        if self.in_flight {
            self.poll_after = true;
            false
        } else {
            self.tick()
        }
    }

    /// A snapshot request finished.
    pub fn polled(&mut self, now: Instant, result: Result<Snapshot, BackendError>) -> bool {
        self.in_flight = false;
        let busy = matches!(&result, Ok(snapshot) if snapshot.busy);
        let state = std::mem::replace(&mut self.state, State::Connecting);
        self.state = state.after(result, now.saturating_duration_since(self.started));
        if std::mem::take(&mut self.poll_after) {
            // This reply was requested before the refresh, so its `busy` says nothing yet.
            // Or the popup opened meanwhile, and spending is still to be asked for.
            return self.tick();
        }
        if !busy {
            self.fast_until = None;
        }
        false
    }
}

/// What the panel button shows (plan §4.2).
#[derive(Debug, Clone, PartialEq)]
enum Panel<'a> {
    /// The app icon, with an optional badge in its corner.
    Icon(Option<Badge>),
    /// Bar entries or the summary, after a `!` when the snapshot is stale. Never [`Bar::Empty`].
    Bar {
        snapshot: &'a Snapshot,
        bar: Bar<'a>,
        stale: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Badge {
    /// `Unavailable` or `Incompatible`.
    Warning,
    /// Connected, but the snapshot is stale.
    Stale,
}

impl<'a> Panel<'a> {
    /// `hidden`: the providers the user turned off on the popup's panel page.
    fn new(state: &'a State, horizontal: bool, hidden: &[String]) -> Self {
        let snapshot = match state {
            State::Connecting => return Self::Icon(None),
            State::Unavailable(_) | State::Incompatible => return Self::Icon(Some(Badge::Warning)),
            State::Connected(snapshot) => snapshot,
        };
        match snapshot.bar(hidden) {
            bar @ (Bar::Entries { .. } | Bar::Summary(_)) if horizontal => Self::Bar {
                snapshot,
                bar,
                stale: snapshot.stale,
            },
            _ => Self::Icon(snapshot.stale.then_some(Badge::Stale)),
        }
    }
}

pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    backend: Backend,
    connection: Connection,
    popup: Option<Id>,
    /// Checked on connect and whenever the popup opens.
    autostart: Autostart,
    tray: TrayPrompt,
    /// `None` when the config folder can't be opened: then nothing is remembered.
    config_handler: Option<cosmic::cosmic_config::Config>,
    config: Config,
    /// The popup's selected tab, kept while the applet runs. `None` selects the first entry.
    provider: Option<String>,
    /// The popup shows the panel page instead of the tabs, until it closes.
    panel_page: bool,
    /// The panel page's style choice, kept in step with `config.panel_style`.
    styles: segmented_button::SingleSelectModel,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    /// `Esc` in the popup.
    ClosePopup,
    PopupClosed(Id),
    Tick,
    Polled(Result<Snapshot, BackendError>),
    Refresh,
    Refreshed(Result<(), BackendError>),
    /// A popup header button: ask the desktop app to open one of its windows.
    Open(backend::Window),
    /// A provider's tab in the popup.
    SelectProvider(String),
    /// A provider's toggle on the popup's panel page.
    ShowInPanel(String, bool),
    /// The panel button in the popup's header, or the back button on the panel page.
    TogglePanelPage,
    /// A segment of the style choice on the panel page.
    ChoosePanelStyle(segmented_button::Entity),
    /// The left or right arrow in the popup.
    SwitchProvider(popup::Step),
    /// **Check again** in the `Unavailable` popup.
    CheckAgain,
    /// A link in the guidance: one of the `guidance` URLs.
    OpenUrl(&'static str),
    AutostartChecked(Result<bool, BackendError>),
    /// **Turn on** in the autostart banner.
    TurnOnAutostart,
    AutostartEnabled(Result<bool, BackendError>),
    /// **Hide tray icon** in the tray prompt.
    HideTray,
    TrayHidden(Result<(), BackendError>),
    /// **Keep it** in the tray prompt.
    KeepTray,
    /// The stored config changed, for example from the applet on another panel.
    ConfigChanged(Config),
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;

    type Flags = ();

    type Message = Message;

    const APP_ID: &'static str = "io.github.leonardo_panseri.cosmic-ext-applet-codexbar";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let (config_handler, config) = config::load(Self::APP_ID);
        let mut app = AppModel {
            core,
            backend: Backend::from_env(),
            connection: Connection::new(Instant::now()),
            popup: None,
            autostart: Autostart::default(),
            tray: TrayPrompt::new(config.tray_prompt_dismissed),
            styles: popup::style_model(config.panel_style),
            config_handler,
            config,
            provider: None,
            panel_page: false,
        };
        let task = if app.connection.tick() {
            app.poll()
        } else {
            Task::none()
        };
        (app, task)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        let poll = cosmic::iced::time::every(self.connection.interval(Instant::now()))
            .map(|_| Message::Tick);
        let config = self
            .core
            .watch_config::<Config>(Self::APP_ID)
            .map(|update| Message::ConfigChanged(update.config));
        if self.popup.is_some() {
            Subscription::batch([poll, config, keyboard::listen().filter_map(popup::shortcut)])
        } else {
            Subscription::batch([poll, config])
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let applet = &self.core.applet;
        let panel = Panel::new(
            self.connection.state(),
            applet.is_horizontal(),
            &self.config.hidden_providers,
        );
        let button = match panel {
            Panel::Icon(badge) => applet.button_from_element(self.app_icon(badge), true),
            Panel::Bar {
                snapshot,
                bar,
                stale,
            } => {
                // As `applet.text_button`, which takes only a single text.
                let (major_padding, minor_padding) = applet.suggested_padding(true);
                let height = applet.suggested_size(true).1 + 2 * minor_padding;
                widget::button::custom(
                    widget::layer_container(panel_bar(
                        applet,
                        snapshot,
                        bar,
                        stale,
                        self.config.panel_style,
                    ))
                    .center_y(Length::Fixed(f32::from(height))),
                )
                .padding([0, major_padding])
                .class(cosmic::theme::Button::AppletIcon)
            }
        }
        // On press, not release, like COSMIC's own applets.
        .on_press_down(Message::TogglePopup);

        applet
            .autosize_window(widget::mouse_area(button).on_middle_press(Message::Refresh))
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let content = match self.connection.state() {
            State::Connected(snapshot) => popup::view(
                snapshot,
                self.provider.as_deref(),
                popup::PanelSettings {
                    open: self.panel_page,
                    hidden: &self.config.hidden_providers,
                    styles: &self.styles,
                },
                self.autostart.banner(),
                self.tray.banner(),
            ),
            State::Connecting => guidance::connecting(),
            State::Unavailable(why) => guidance::unavailable(*why),
            State::Incompatible => guidance::incompatible(),
        };
        self.core
            .applet
            .popup_container(popup::backdrop(content))
            .into()
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Tick => {
                if self.connection.tick() {
                    return self.poll();
                }
            }
            Message::Polled(result) => {
                let was_connected = self.connection.state().is_connected();
                let poll = if self.connection.polled(Instant::now(), result) {
                    self.poll()
                } else {
                    Task::none()
                };
                // On connect, as well as when the popup opens (plan D7).
                let autostart = if !was_connected && self.connection.state().is_connected() {
                    self.check_autostart()
                } else {
                    Task::none()
                };
                return Task::batch([poll, autostart]);
            }
            Message::Refresh => {
                let backend = self.backend.clone();
                return Task::perform(async move { backend.refresh().await }, |result| {
                    cosmic::Action::App(Message::Refreshed(result))
                });
            }
            Message::Refreshed(result) => {
                if self.connection.refreshed(Instant::now(), result.is_ok()) {
                    return self.poll();
                }
            }
            Message::Open(window) => {
                let backend = self.backend.clone();
                // A failure shows up as `Unavailable` on the next poll.
                let open = Task::future(async move { backend.open(window).await }).discard();
                return Task::batch([open, self.close_popup()]);
            }
            Message::SelectProvider(provider) => self.provider = Some(provider),
            Message::ShowInPanel(..) | Message::TogglePanelPage | Message::ChoosePanelStyle(_) => {
                self.update_panel(message);
            }
            Message::SwitchProvider(step) => {
                if let State::Connected(snapshot) = self.connection.state() {
                    self.provider = popup::adjacent(snapshot, self.provider.as_deref(), step)
                        .map(str::to_owned);
                }
            }
            Message::CheckAgain => {
                if self.connection.check_again() {
                    return self.poll();
                }
            }
            Message::OpenUrl(url) => return Task::batch([open_url(url), self.close_popup()]),
            Message::AutostartChecked(_)
            | Message::TurnOnAutostart
            | Message::AutostartEnabled(_)
            | Message::HideTray
            | Message::TrayHidden(_)
            | Message::KeepTray
            | Message::ConfigChanged(_) => return self.update_banners(message),
            Message::ClosePopup => return self.close_popup(),
            Message::TogglePopup => {
                return if self.popup.is_some() {
                    self.close_popup()
                } else {
                    self.open_popup()
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

impl AppModel {
    /// Sends the snapshot request that [`Connection`] just asked for.
    fn poll(&mut self) -> Task<cosmic::Action<Message>> {
        let spending = self.connection.take_spending();
        let backend = self.backend.clone();
        Task::perform(async move { backend.snapshot(spending).await }, |result| {
            cosmic::Action::App(Message::Polled(result))
        })
    }

    /// The autostart banner and the tray prompt.
    fn update_banners(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::AutostartChecked(result) => self.autostart.checked(result),
            Message::TurnOnAutostart => {
                if self.autostart.turn_on() {
                    let backend = self.backend.clone();
                    return Task::perform(
                        async move { backend.enable_autostart().await },
                        |result| cosmic::Action::App(Message::AutostartEnabled(result)),
                    );
                }
            }
            Message::AutostartEnabled(result) => {
                self.autostart.enable_replied(&result);
                return self.check_autostart();
            }
            Message::HideTray => {
                if self.tray.hide() {
                    let backend = self.backend.clone();
                    return Task::perform(async move { backend.hide_tray().await }, |result| {
                        cosmic::Action::App(Message::TrayHidden(result))
                    });
                }
            }
            Message::TrayHidden(result) => {
                if self.tray.hide_replied(&result) {
                    self.store_tray_dismissal();
                }
            }
            Message::KeepTray => {
                if self.tray.keep() {
                    self.store_tray_dismissal();
                }
            }
            Message::ConfigChanged(config) => {
                self.tray.stored(config.tray_prompt_dismissed);
                popup::select_style(&mut self.styles, config.panel_style);
                self.config = config;
            }
            _ => {}
        }
        Task::none()
    }

    fn check_autostart(&self) -> Task<cosmic::Action<Message>> {
        let backend = self.backend.clone();
        Task::perform(async move { backend.autostart_status().await }, |result| {
            cosmic::Action::App(Message::AutostartChecked(result))
        })
    }

    /// Remembers that the tray prompt was answered, so it doesn't come back (plan D8).
    fn store_tray_dismissal(&mut self) {
        let Some(handler) = &self.config_handler else {
            return;
        };
        if let Err(why) = self.config.set_tray_prompt_dismissed(handler, true) {
            eprintln!("can't store the tray prompt's answer: {why}");
        }
    }

    /// Stores a provider's toggle from the panel page. The config watch then tells the applet on
    /// any other panel.
    fn show_in_panel(&mut self, provider: &str, shown: bool) {
        let hidden = with_shown(&self.config.hidden_providers, provider, shown);
        let Some(handler) = &self.config_handler else {
            self.config.hidden_providers = hidden;
            return;
        };
        if let Err(why) = self.config.set_hidden_providers(handler, hidden) {
            eprintln!("can't store which providers the panel shows: {why}");
        }
    }

    /// The messages from the popup's panel page.
    fn update_panel(&mut self, message: Message) {
        match message {
            Message::ShowInPanel(provider, shown) => self.show_in_panel(&provider, shown),
            Message::TogglePanelPage => self.panel_page = !self.panel_page,
            Message::ChoosePanelStyle(item) => {
                if let Some(style) = popup::style_of(&self.styles, item) {
                    self.styles.activate(item);
                    self.set_panel_style(style);
                }
            }
            _ => {}
        }
    }

    fn set_panel_style(&mut self, style: PanelStyle) {
        let Some(handler) = &self.config_handler else {
            self.config.panel_style = style;
            return;
        };
        if let Err(why) = self.config.set_panel_style(handler, style) {
            eprintln!("can't store the panel style: {why}");
        }
    }

    fn open_popup(&mut self) -> Task<cosmic::Action<Message>> {
        let new_id = Id::unique();
        self.popup.replace(new_id);
        self.panel_page = false;
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
            .min_height(100.0)
            .max_height(1080.0);
        let snapshot = if self.connection.popup_opened() {
            self.poll()
        } else {
            Task::none()
        };
        self.autostart.popup_opened();
        self.tray.popup_opened();
        // Through libcosmic's surface action, not iced's `get_popup`: libcosmic then knows
        // the surface is a popup and asks the compositor to blur it under frosted glass.
        // A popup it doesn't know gets the translucent background without the blur.
        // With no view of its own here, it still shows `view_window`.
        let open = cosmic::surface::surface_task(cosmic::surface::action::app_popup(
            |_: &Self| LiveSettings::default(),
            move |_: &mut Self| popup_settings.clone(),
            None,
        ));
        Task::batch([open, snapshot, self.check_autostart()])
    }

    fn close_popup(&mut self) -> Task<cosmic::Action<Message>> {
        self.popup.take().map_or_else(Task::none, destroy_popup)
    }

    fn icon_size(&self) -> u16 {
        self.core.applet.suggested_size(true).0
    }

    /// The app icon, with `badge` in its bottom-right corner.
    fn app_icon(&self, badge: Option<Badge>) -> Element<'_, Message> {
        let size = self.icon_size();
        let app = icon::icon(icons::app()).size(size);
        let Some(badge) = badge else {
            return app.into();
        };
        let badge_size = (size / 2).max(8);
        let badge = match badge {
            Badge::Warning => icon::from_name("dialog-warning-symbolic")
                .size(badge_size)
                .icon()
                .class(cosmic::theme::Svg::Custom(Rc::new(|theme| svg::Style {
                    color: Some(theme.cosmic().warning_color().into()),
                }))),
            Badge::Stale => icon::from_name("emblem-important-symbolic")
                .size(badge_size)
                .icon(),
        }
        .size(badge_size);
        stack![
            app,
            widget::container(badge)
                .align_right(Length::Fill)
                .align_bottom(Length::Fill),
        ]
        .width(Length::Fixed(f32::from(size)))
        .height(Length::Fixed(f32::from(size)))
        .into()
    }
}

/// The horizontal panel text: each bar entry's logo (or tag), then its windows as small bars and
/// its text as `shows` says, separated by `·`, then `+N` for entries that didn't fit. A provider
/// without windows, for example one that failed, shows its text even with bars only.
pub fn panel_bar<'a>(
    applet: &'a cosmic::applet::Context,
    snapshot: &'a Snapshot,
    bar: Bar<'a>,
    stale: bool,
    shows: PanelStyle,
) -> Element<'a, Message> {
    let icon_size = applet.suggested_size(true).0;
    let mut row = widget::Row::new().spacing(4).align_y(Alignment::Center);
    if stale {
        row = row.push(applet.text("!"));
    }
    match bar {
        Bar::Entries { entries, more } => {
            for (i, entry) in entries.iter().enumerate() {
                if i > 0 {
                    row = row.push(applet.text("·"));
                }
                row = row.push(match icons::provider(&entry.provider) {
                    Some(logo) => Element::from(icon::icon(logo).size(icon_size)),
                    None => applet.text(entry.tag.as_str()).into(),
                });
                let meters = shows
                    .bars()
                    .then(|| meters(snapshot.meters(&entry.provider), icon_size))
                    .flatten();
                let text = shows.percent() || meters.is_none();
                row = row.push_maybe(meters);
                if text {
                    row = row.push(applet.text(entry.text.as_str()));
                }
            }
            if more > 0 {
                row = row.push(applet.text(format!("+{more}")));
            }
        }
        Bar::Summary(summary) => row = row.push(applet.text(summary)),
        Bar::Empty => {}
    }
    row.into()
}

/// A provider's windows as small stacked bars, sized after the panel's icons. `None` without
/// windows, for example when the provider failed.
fn meters<'a>(meters: Vec<Meter>, icon_size: u16) -> Option<Element<'a, Message>> {
    if meters.is_empty() {
        return None;
    }
    let (girth, gap) = meter_size(icon_size, meters.len());
    let width = (f32::from(icon_size) * 1.5).round();
    let column = meters
        .into_iter()
        .fold(widget::Column::new().spacing(gap), |column, value| {
            column.push(meter(value, width, girth))
        });
    Some(column.into())
}

/// The thickness of each bar and the gap between them, so `count` bars fit in an icon's height:
/// at 16 px, three 4 px bars with 2 px gaps. Fewer bars stay as thin as that.
#[allow(clippy::cast_precision_loss)] // At most three bars.
fn meter_size(icon_size: u16, count: usize) -> (f32, f32) {
    let size = f32::from(icon_size);
    let gap = (size / 8.0).round().max(1.0);
    let count = count.max(1) as f32;
    let girth = ((size - gap * (count - 1.0)) / count)
        .floor()
        .min((size / 4.0).round())
        .max(2.0);
    (girth, gap)
}

/// `hidden` with `provider` taken out when `shown`, or added when not.
pub fn with_shown(hidden: &[String], provider: &str, shown: bool) -> Vec<String> {
    let mut hidden: Vec<String> = hidden
        .iter()
        .filter(|hidden| *hidden != provider)
        .cloned()
        .collect();
    if !shown {
        hidden.push(provider.to_owned());
    }
    hidden
}

/// One small bar: a track as wide as `width`, filled to `meter.value` percent in the accent
/// colour, or the destructive colour when the window has a warning.
fn meter<'a>(meter: Meter, width: f32, girth: f32) -> Element<'a, Message> {
    let filled = popup::portion(popup::percent(meter.value));
    let fill = (filled > 0).then(|| {
        widget::container(widget::Space::new())
            .width(Length::FillPortion(filled))
            .height(Length::Fill)
            .class(cosmic::theme::Container::custom(move |theme| {
                let cosmic = theme.cosmic();
                let color = if meter.warning {
                    cosmic.destructive_color()
                } else {
                    cosmic.accent_color()
                };
                widget::container::Style {
                    background: Some(Color::from(color).into()),
                    border: Border {
                        radius: (girth / 2.0).into(),
                        ..Border::default()
                    },
                    ..widget::container::Style::default()
                }
            }))
    });
    // Without the zero-width part, which iced would lay out as filling the row.
    let row = widget::Row::new()
        .push_maybe(fill)
        .push_maybe(popup::spacer(1000 - filled));
    widget::container(row)
        .width(Length::Fixed(width))
        .height(Length::Fixed(girth))
        .class(cosmic::theme::Container::custom(move |theme| {
            let mut track: Color = theme.cosmic().on_bg_color().into();
            track.a = 0.25;
            widget::container::Style {
                background: Some(track.into()),
                border: Border {
                    radius: (girth / 2.0).into(),
                    ..Border::default()
                },
                ..widget::container::Style::default()
            }
        }))
        .into()
}

/// Opens a web page with `xdg-open`. Inside the Flatpak that is flatpak-xdg-utils, which goes
/// through the `OpenURI` portal (plan §4.4). Waiting for it reaps the process.
fn open_url(url: &'static str) -> Task<cosmic::Action<Message>> {
    Task::future(async move {
        match tokio::process::Command::new("xdg-open")
            .arg(url)
            .status()
            .await
        {
            Ok(status) if !status.success() => eprintln!("xdg-open {url} failed: {status}"),
            Ok(_) => {}
            Err(why) => eprintln!("can't run xdg-open: {why}"),
        }
    })
    .discard()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{self, Incompatible};

    const SECOND: Duration = Duration::from_secs(1);

    fn fixture(json: &str) -> Snapshot {
        snapshot::parse(json.as_bytes()).unwrap()
    }

    fn two_providers() -> Snapshot {
        fixture(include_str!(
            "../tests/fixtures/snapshot-two-providers.json"
        ))
    }

    fn busy(busy: bool) -> Snapshot {
        Snapshot {
            busy,
            ..two_providers()
        }
    }

    /// A connection whose startup poll is already in flight, as after `init`.
    fn started() -> (Connection, Instant) {
        let t0 = Instant::now();
        let mut connection = Connection::new(t0);
        assert!(connection.tick());
        (connection, t0)
    }

    /// One whole poll at `now`: tick, then the reply.
    fn poll_at(connection: &mut Connection, now: Instant, result: Result<Snapshot, BackendError>) {
        assert!(connection.tick(), "a poll was already in flight");
        assert!(!connection.polled(now, result));
    }

    #[test]
    fn connecting_until_the_first_snapshot() {
        let (mut connection, t0) = started();
        assert_eq!(connection.state(), &State::Connecting);
        assert!(!connection.state().is_connected());
        assert!(!connection.polled(t0, Ok(two_providers())));
        assert_eq!(connection.state(), &State::Connected(two_providers()));
        assert!(connection.state().is_connected());
    }

    #[test]
    fn check_again_polls_now_or_after_the_poll_in_flight() {
        let (mut connection, t0) = started();
        assert!(!connection.check_again(), "still one request in flight");
        assert!(connection.polled(t0 + GRACE, Err(BackendError::NotRunning)));
        assert!(!connection.polled(t0 + GRACE, Err(BackendError::NotRunning)));
        assert!(connection.check_again());
        assert!(!connection.take_spending());
        assert_eq!(connection.interval(t0 + GRACE), POLL);
    }

    #[test]
    fn failures_in_the_grace_period_stay_connecting() {
        let (mut connection, t0) = started();
        connection.polled(t0, Err(BackendError::NotRunning));
        assert_eq!(connection.state(), &State::Connecting);
        poll_at(&mut connection, t0 + 5 * SECOND, Err(BackendError::Timeout));
        assert_eq!(connection.state(), &State::Connecting);
        poll_at(&mut connection, t0 + GRACE, Err(BackendError::NotRunning));
        assert_eq!(
            connection.state(),
            &State::Unavailable(Unavailable::NotRunning)
        );
    }

    #[test]
    fn unavailable_remembers_why() {
        let (mut connection, t0) = started();
        connection.polled(t0 + GRACE, Err(BackendError::NotResponding));
        assert_eq!(
            connection.state(),
            &State::Unavailable(Unavailable::NotResponding)
        );
        // A desktop app that hangs is not responding either.
        poll_at(
            &mut connection,
            t0 + 15 * SECOND,
            Err(BackendError::Timeout),
        );
        assert_eq!(
            connection.state(),
            &State::Unavailable(Unavailable::NotResponding)
        );
        // A clean `--quit` removes the socket.
        poll_at(
            &mut connection,
            t0 + 20 * SECOND,
            Err(BackendError::NotRunning),
        );
        assert_eq!(
            connection.state(),
            &State::Unavailable(Unavailable::NotRunning)
        );
    }

    #[test]
    fn incompatible_replies_skip_the_grace_period() {
        for error in [
            BackendError::Incompatible(Incompatible::SchemaVersion(Some(2))),
            BackendError::Incompatible(Incompatible::InvalidJson("eof".into())),
            BackendError::TooLarge,
            BackendError::Rejected("Unknown command".into()),
        ] {
            let (mut connection, t0) = started();
            connection.polled(t0, Err(error));
            assert_eq!(connection.state(), &State::Incompatible);
        }
    }

    #[test]
    fn losing_the_backend_after_connecting_shows_at_once_and_recovers() {
        let (mut connection, t0) = started();
        connection.polled(t0, Ok(two_providers()));
        // Well within what would have been the grace period.
        poll_at(
            &mut connection,
            t0 + 5 * SECOND,
            Err(BackendError::NotRunning),
        );
        assert_eq!(
            connection.state(),
            &State::Unavailable(Unavailable::NotRunning)
        );
        poll_at(&mut connection, t0 + 10 * SECOND, Ok(two_providers()));
        assert_eq!(connection.state(), &State::Connected(two_providers()));
        poll_at(
            &mut connection,
            t0 + 15 * SECOND,
            Err(BackendError::Incompatible(Incompatible::SchemaVersion(
                Some(2),
            ))),
        );
        assert_eq!(connection.state(), &State::Incompatible);
    }

    #[test]
    fn ticks_are_skipped_while_a_poll_is_in_flight() {
        let (mut connection, t0) = started();
        assert!(!connection.tick());
        assert!(!connection.tick());
        assert!(!connection.polled(t0 + 7 * SECOND, Ok(two_providers())));
        assert!(connection.tick());
    }

    #[test]
    fn polls_every_five_seconds_by_default() {
        let (mut connection, t0) = started();
        assert_eq!(connection.interval(t0), POLL);
        connection.polled(t0 + GRACE, Err(BackendError::NotRunning));
        assert_eq!(connection.interval(t0 + GRACE), POLL);
        // A busy snapshot the applet didn't ask for doesn't speed polling up.
        poll_at(&mut connection, t0 + 15 * SECOND, Ok(busy(true)));
        assert_eq!(connection.interval(t0 + 15 * SECOND), POLL);
    }

    #[test]
    fn refresh_polls_at_once_then_fast_while_busy() {
        let (mut connection, t0) = started();
        connection.polled(t0, Ok(two_providers()));

        let t1 = t0 + 2 * SECOND;
        assert!(connection.refreshed(t1, true));
        assert_eq!(connection.interval(t1), FAST_POLL);
        assert!(!connection.polled(t1, Ok(busy(true))));
        poll_at(&mut connection, t1 + SECOND, Ok(busy(true)));
        assert_eq!(connection.interval(t1 + SECOND), FAST_POLL);

        poll_at(&mut connection, t1 + 2 * SECOND, Ok(busy(false)));
        assert_eq!(connection.interval(t1 + 2 * SECOND), POLL);
    }

    #[test]
    fn refresh_during_a_poll_polls_again_when_it_returns() {
        let (mut connection, t0) = started();
        assert!(!connection.refreshed(t0, true));
        // This reply was requested before the refresh: its `busy: false` is old news.
        assert!(connection.polled(t0, Ok(busy(false))));
        assert_eq!(connection.interval(t0), FAST_POLL);
        assert!(!connection.polled(t0 + SECOND, Ok(busy(true))));
        assert_eq!(connection.interval(t0 + SECOND), FAST_POLL);
    }

    #[test]
    fn fast_polling_stops_after_the_cap() {
        let (mut connection, t0) = started();
        connection.polled(t0, Ok(two_providers()));
        assert!(connection.refreshed(t0, true));
        assert!(!connection.polled(t0, Ok(busy(true))));
        let mut now = t0;
        while now < t0 + FAST_POLL_CAP {
            assert_eq!(connection.interval(now), FAST_POLL);
            poll_at(&mut connection, now, Ok(busy(true)));
            now += FAST_POLL;
        }
        assert_eq!(connection.interval(t0 + FAST_POLL_CAP), POLL);
    }

    #[test]
    fn rejected_or_failed_refresh_polls_once_at_normal_speed() {
        let (mut connection, t0) = started();
        connection.polled(t0 + GRACE, Err(BackendError::NotRunning));
        assert!(connection.refreshed(t0 + GRACE, false));
        assert_eq!(connection.interval(t0 + GRACE), POLL);
    }

    #[test]
    fn only_polls_after_the_popup_opens_ask_for_spending() {
        let (mut connection, t0) = started();
        assert!(!connection.take_spending());
        connection.polled(t0, Ok(two_providers()));

        assert!(connection.popup_opened());
        assert!(connection.take_spending());
        assert!(!connection.polled(t0 + SECOND, Ok(two_providers())));
        assert!(connection.tick());
        assert!(!connection.take_spending());
    }

    #[test]
    fn popup_opening_during_a_poll_asks_for_spending_when_it_returns() {
        let (mut connection, t0) = started();
        assert!(!connection.popup_opened());
        assert!(!connection.tick(), "still one request in flight");
        // The startup poll didn't ask for spending, so poll again at once.
        assert!(connection.polled(t0, Ok(two_providers())));
        assert!(connection.take_spending());
        assert!(!connection.polled(t0, Ok(two_providers())));
        assert!(!connection.take_spending());
    }

    #[test]
    fn panel_shows_the_app_icon_until_connected() {
        for horizontal in [true, false] {
            assert_eq!(
                Panel::new(&State::Connecting, horizontal, &[]),
                Panel::Icon(None)
            );
            for state in [
                State::Unavailable(Unavailable::NotRunning),
                State::Unavailable(Unavailable::NotResponding),
                State::Incompatible,
            ] {
                assert_eq!(
                    Panel::new(&state, horizontal, &[]),
                    Panel::Icon(Some(Badge::Warning))
                );
            }
        }
    }

    #[test]
    fn horizontal_panel_shows_the_bar() {
        let state = State::Connected(fixture(include_str!(
            "../tests/fixtures/snapshot-provider-error.json"
        )));
        let Panel::Bar {
            bar: Bar::Entries { entries, more },
            stale: false,
            ..
        } = Panel::new(&state, true, &[])
        else {
            panic!("{:?}", Panel::new(&state, true, &[]));
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(more, 1);

        let summary_only = State::Connected(Snapshot {
            summary: "CL 90%".into(),
            ..Snapshot::default()
        });
        assert!(matches!(
            Panel::new(&summary_only, true, &[]),
            Panel::Bar {
                bar: Bar::Summary("CL 90%"),
                stale: false,
                ..
            }
        ));
    }

    #[test]
    fn hiding_every_provider_shows_the_app_icon() {
        let state = State::Connected(two_providers());
        let hidden = ["claude".to_owned(), "codex".to_owned()];
        assert_eq!(Panel::new(&state, true, &hidden), Panel::Icon(None));
        assert!(matches!(
            Panel::new(&state, true, &hidden[..1]),
            Panel::Bar {
                bar: Bar::Entries { more: 0, .. },
                ..
            }
        ));
    }

    #[test]
    fn bars_fit_in_an_icon() {
        assert_eq!(meter_size(16, 3), (4.0, 2.0));
        assert_eq!(meter_size(16, 2), (4.0, 2.0));
        assert_eq!(meter_size(24, 3), (6.0, 3.0));
        assert_eq!(meter_size(32, 3), (8.0, 4.0));
        for size in [12, 16, 20, 24, 32] {
            let (girth, gap) = meter_size(size, 3);
            assert!(3.0 * girth + 2.0 * gap <= f32::from(size), "{size}");
        }
    }

    #[test]
    fn show_in_panel_adds_and_removes_hidden_providers() {
        let hidden = with_shown(&[], "claude", false);
        assert_eq!(hidden, ["claude"]);
        // Twice is still once.
        assert_eq!(with_shown(&hidden, "claude", false), ["claude"]);
        let both = with_shown(&hidden, "codex", false);
        assert_eq!(both, ["claude", "codex"]);
        assert_eq!(with_shown(&both, "claude", true), ["codex"]);
        assert_eq!(with_shown(&[], "gemini", true), Vec::<String>::new());
    }

    #[test]
    fn stale_snapshots_are_marked() {
        let state = State::Connected(Snapshot {
            stale: true,
            ..two_providers()
        });
        assert!(matches!(
            Panel::new(&state, true, &[]),
            Panel::Bar { stale: true, .. }
        ));
        assert_eq!(
            Panel::new(&state, false, &[]),
            Panel::Icon(Some(Badge::Stale))
        );
    }

    #[test]
    fn vertical_panel_and_empty_snapshots_show_the_app_icon() {
        let connected = State::Connected(two_providers());
        assert_eq!(Panel::new(&connected, false, &[]), Panel::Icon(None));

        // The first seconds after the desktop app starts.
        let empty = State::Connected(Snapshot::default());
        assert_eq!(Panel::new(&empty, true, &[]), Panel::Icon(None));
        let empty_stale = State::Connected(Snapshot {
            stale: true,
            ..Snapshot::default()
        });
        assert_eq!(
            Panel::new(&empty_stale, true, &[]),
            Panel::Icon(Some(Badge::Stale))
        );
    }
}
