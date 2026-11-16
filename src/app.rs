// SPDX-License-Identifier: MPL-2.0

//! The panel button, the poll loop and what the applet knows about the backend
//! (`docs/implementation-plan.md` D7, §4.1 and §4.2). The `Connected` popup is in [`crate::popup`];
//! the other states keep a placeholder until P4.

use std::rc::Rc;
use std::time::{Duration, Instant};

use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::widget::{stack, svg};
use cosmic::iced::{Alignment, Length, Limits, Subscription, keyboard, window::Id};
use cosmic::prelude::*;
use cosmic::widget::{self, icon};

use crate::backend::{self, Backend, BackendError};
use crate::snapshot::{Bar, Snapshot};
use crate::{fl, icons, popup};

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
#[derive(Debug, Clone, Copy, PartialEq)]
enum Panel<'a> {
    /// The app icon, with an optional badge in its corner.
    Icon(Option<Badge>),
    /// Bar entries or the summary, after a `!` when the snapshot is stale. Never [`Bar::Empty`].
    Bar { bar: Bar<'a>, stale: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Badge {
    /// `Unavailable` or `Incompatible`.
    Warning,
    /// Connected, but the snapshot is stale.
    Stale,
}

impl<'a> Panel<'a> {
    fn new(state: &'a State, horizontal: bool) -> Self {
        let snapshot = match state {
            State::Connecting => return Self::Icon(None),
            State::Unavailable(_) | State::Incompatible => return Self::Icon(Some(Badge::Warning)),
            State::Connected(snapshot) => snapshot,
        };
        match snapshot.bar() {
            bar @ (Bar::Entries { .. } | Bar::Summary(_)) if horizontal => Self::Bar {
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
    /// Whether the desktop app starts at login, as of the last time the popup opened. `None`
    /// until then, or when the desktop app couldn't say.
    #[allow(dead_code)] // The autostart banner (P4) reads it.
    autostart: Option<bool>,
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
    /// A popup footer button: ask the desktop app to open one of its windows.
    Open(backend::Window),
    AutostartChecked(Result<bool, BackendError>),
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
        let mut app = AppModel {
            core,
            backend: Backend::from_env(),
            connection: Connection::new(Instant::now()),
            popup: None,
            autostart: None,
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
        if self.popup.is_some() {
            Subscription::batch([poll, keyboard::listen().filter_map(popup::shortcut)])
        } else {
            poll
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let applet = &self.core.applet;
        let button = match Panel::new(self.connection.state(), applet.is_horizontal()) {
            Panel::Icon(badge) => applet.button_from_element(self.app_icon(badge), true),
            Panel::Bar { bar, stale } => {
                // As `applet.text_button`, which takes only a single text.
                let (major_padding, minor_padding) = applet.suggested_padding(true);
                let height = applet.suggested_size(true).1 + 2 * minor_padding;
                widget::button::custom(
                    widget::layer_container(self.bar(bar, stale))
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
        let status = match self.connection.state() {
            State::Connected(snapshot) => {
                return self
                    .core
                    .applet
                    .popup_container(popup::view(snapshot))
                    .into();
            }
            // Placeholders until the guidance views (P4).
            State::Connecting => fl!("connecting"),
            State::Unavailable(Unavailable::NotRunning) => fl!("not-running"),
            State::Unavailable(Unavailable::NotResponding) => fl!("not-responding"),
            State::Incompatible => fl!("incompatible"),
        };
        let content = cosmic::iced::widget::column![
            widget::text::heading(fl!("app-title")),
            widget::text::body(status),
        ]
        .spacing(8)
        .padding(16);

        self.core.applet.popup_container(content).into()
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Tick => {
                if self.connection.tick() {
                    return self.poll();
                }
            }
            Message::Polled(result) => {
                if self.connection.polled(Instant::now(), result) {
                    return self.poll();
                }
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
            Message::AutostartChecked(result) => self.autostart = result.ok(),
            Message::ClosePopup => return self.close_popup(),
            Message::TogglePopup => {
                return if self.popup.is_some() {
                    self.close_popup()
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
                        .min_height(100.0)
                        .max_height(1080.0);
                    let snapshot = if self.connection.popup_opened() {
                        self.poll()
                    } else {
                        Task::none()
                    };
                    let backend = self.backend.clone();
                    let autostart =
                        Task::perform(async move { backend.autostart_status().await }, |result| {
                            cosmic::Action::App(Message::AutostartChecked(result))
                        });
                    Task::batch([get_popup(popup_settings), snapshot, autostart])
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

    /// The horizontal panel text: each bar entry's logo (or tag) and text, separated by `·`,
    /// then `+N` for entries that didn't fit.
    fn bar<'a>(&'a self, bar: Bar<'a>, stale: bool) -> Element<'a, Message> {
        let applet = &self.core.applet;
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
                        Some(logo) => Element::from(icon::icon(logo).size(self.icon_size())),
                        None => applet.text(entry.tag.as_str()).into(),
                    });
                    row = row.push(applet.text(entry.text.as_str()));
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
        assert!(!connection.polled(t0, Ok(two_providers())));
        assert_eq!(connection.state(), &State::Connected(two_providers()));
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
                Panel::new(&State::Connecting, horizontal),
                Panel::Icon(None)
            );
            for state in [
                State::Unavailable(Unavailable::NotRunning),
                State::Unavailable(Unavailable::NotResponding),
                State::Incompatible,
            ] {
                assert_eq!(
                    Panel::new(&state, horizontal),
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
        } = Panel::new(&state, true)
        else {
            panic!("{:?}", Panel::new(&state, true));
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(more, 1);

        let summary_only = State::Connected(Snapshot {
            summary: "CL 90%".into(),
            ..Snapshot::default()
        });
        assert_eq!(
            Panel::new(&summary_only, true),
            Panel::Bar {
                bar: Bar::Summary("CL 90%"),
                stale: false
            }
        );
    }

    #[test]
    fn stale_snapshots_are_marked() {
        let state = State::Connected(Snapshot {
            stale: true,
            ..two_providers()
        });
        assert!(matches!(
            Panel::new(&state, true),
            Panel::Bar { stale: true, .. }
        ));
        assert_eq!(Panel::new(&state, false), Panel::Icon(Some(Badge::Stale)));
    }

    #[test]
    fn vertical_panel_and_empty_snapshots_show_the_app_icon() {
        let connected = State::Connected(two_providers());
        assert_eq!(Panel::new(&connected, false), Panel::Icon(None));

        // The first seconds after the desktop app starts.
        let empty = State::Connected(Snapshot::default());
        assert_eq!(Panel::new(&empty, true), Panel::Icon(None));
        let empty_stale = State::Connected(Snapshot {
            stale: true,
            ..Snapshot::default()
        });
        assert_eq!(
            Panel::new(&empty_stale, true),
            Panel::Icon(Some(Badge::Stale))
        );
    }
}
