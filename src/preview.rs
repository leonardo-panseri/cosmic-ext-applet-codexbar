// SPDX-License-Identifier: MPL-2.0

//! Debug builds only: `CODEXBAR_PREVIEW=tests/fixtures/<file>.json cargo run` shows the
//! panel button's content and the `Connected` popup for that snapshot in an ordinary window, to
//! check the layout without the panel or the desktop app. `CODEXBAR_PREVIEW_PROVIDER=codex`
//! selects that tab first, and `CODEXBAR_PREVIEW_PANEL_PAGE=1` opens the panel page. The header
//! buttons do nothing.
//!
//! `CODEXBAR_PREVIEW_SCREENSHOT=<file>` instead draws a mock top panel over a backdrop, in
//! COSMIC's default dark theme, saves the window's pixels to `<file>` and exits.
//! `CODEXBAR_PREVIEW_HEIGHT` sets the window's height in that mode. The file is `RGBA`, the width
//! and height as little-endian `u32`s, then the pixels;
//! `tools/screenshots.py` turns it into a PNG.

use std::time::Duration;

use cosmic::iced::gradient::Linear;
use cosmic::iced::{Background, Border, Color, Gradient, Length, Limits, Radians, Size, window};
use cosmic::prelude::*;
use cosmic::widget;

use cosmic::widget::segmented_button::SingleSelectModel;

use crate::app::{self, Message};
use crate::config::PanelStyle;
use crate::popup;
use crate::snapshot::{self, Snapshot};

/// As wide as the panel's popup.
const WIDTH: f32 = 376.0;
/// The screenshot's size in logical pixels. Wide enough for app stores, which want 620 px. The
/// saved pixels follow the display's scale; libcosmic's own scale factor doesn't change them.
const SHOT: Size = Size::new(640.0, 700.0);
/// Time for the first frames and the icons to load before the screenshot.
const SHOT_DELAY: Duration = Duration::from_millis(1500);

pub fn run(path: &str) -> cosmic::iced::Result {
    let bytes = std::fs::read(path).unwrap_or_else(|why| panic!("can't read {path}: {why}"));
    let snapshot =
        snapshot::parse(&bytes).unwrap_or_else(|why| panic!("can't parse {path}: {why:?}"));
    let shot = std::env::var("CODEXBAR_PREVIEW_SCREENSHOT").ok();
    let settings = if shot.is_some() {
        let height = std::env::var("CODEXBAR_PREVIEW_HEIGHT")
            .ok()
            .and_then(|height| height.parse().ok())
            .unwrap_or(SHOT.height);
        let size = Size::new(SHOT.width, height);
        cosmic::app::Settings::default()
            .size(size)
            // A window that can't be resized floats instead of being tiled to another size.
            .size_limits(Limits::new(size, size))
            .resizable(None)
            .theme(cosmic::Theme::dark())
    } else {
        cosmic::app::Settings::default().size(Size::new(WIDTH + 40.0, 900.0))
    };
    cosmic::app::run::<Preview>(settings, (snapshot, shot))
}

struct Preview {
    core: cosmic::Core,
    /// Where the screenshot goes, in screenshot mode.
    shot: Option<String>,
    snapshot: Snapshot,
    provider: Option<String>,
    /// As the applet's config, but only in memory.
    hidden: Vec<String>,
    style: PanelStyle,
    styles: SingleSelectModel,
    panel_page: bool,
}

impl cosmic::Application for Preview {
    type Executor = cosmic::executor::Default;
    type Flags = (Snapshot, Option<String>);
    type Message = Message;

    const APP_ID: &'static str = "io.github.leonardo_panseri.cosmic-ext-applet-codexbar.Preview";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        mut core: cosmic::Core,
        (snapshot, shot): Self::Flags,
    ) -> (Self, Task<cosmic::Action<Message>>) {
        let task = match (&shot, core.main_window_id()) {
            (Some(path), Some(id)) => {
                // Only the view: no header bar, padding or rounded corners.
                core.window.show_headerbar = false;
                core.window.content_container = false;
                core.window.sharp_corners = true;
                screenshot(id, path.clone())
            }
            _ => Task::none(),
        };
        let preview = Self {
            core,
            shot,
            snapshot,
            provider: std::env::var("CODEXBAR_PREVIEW_PROVIDER").ok(),
            hidden: Vec::new(),
            style: PanelStyle::default(),
            styles: popup::style_model(PanelStyle::default()),
            panel_page: std::env::var_os("CODEXBAR_PREVIEW_PANEL_PAGE").is_some(),
        };
        (preview, task)
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Message> {
        cosmic::iced::keyboard::listen().filter_map(popup::shortcut)
    }

    fn update(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::SelectProvider(provider) => self.provider = Some(provider),
            Message::ShowInPanel(provider, shown) => {
                self.hidden = app::with_shown(&self.hidden, &provider, shown);
            }
            Message::TogglePanelPage => self.panel_page = !self.panel_page,
            Message::ChoosePanelStyle(item) => {
                if let Some(style) = popup::style_of(&self.styles, item) {
                    self.styles.activate(item);
                    self.style = style;
                }
            }
            Message::SwitchProvider(step) => {
                self.provider = popup::adjacent(&self.snapshot, self.provider.as_deref(), step)
                    .map(str::to_owned);
            }
            _ => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        // The look of the panel's popup container.
        let popup = widget::container(popup::view(
            &self.snapshot,
            self.provider.as_deref(),
            popup::PanelSettings {
                open: self.panel_page,
                hidden: &self.hidden,
                styles: &self.styles,
            },
            None,
            None,
        ))
        .width(Length::Fixed(WIDTH))
        .class(cosmic::theme::Container::custom(|theme| {
            let cosmic = theme.cosmic();
            let background = cosmic.background(theme.transparent);
            widget::container::Style {
                text_color: Some(background.on.into()),
                icon_color: Some(background.on.into()),
                background: Some(Color::from(background.base).into()),
                border: Border {
                    radius: cosmic.corner_radii.radius_m.into(),
                    width: 1.0,
                    color: background.divider.into(),
                },
                ..widget::container::Style::default()
            }
        }));
        let bar = self.snapshot.bar(&self.hidden);
        let panel = widget::container(app::panel_bar(
            &self.core.applet,
            &self.snapshot,
            bar,
            self.snapshot.stale,
            self.style,
        ))
        .padding([6, 12])
        .class(cosmic::theme::Container::Background);
        if self.shot.is_none() {
            return widget::Column::new()
                .spacing(12)
                .padding(20)
                .push(panel)
                .push(popup)
                .into();
        }
        // A top panel with the button near its right end, and the popup under the button.
        let panel = widget::container(
            widget::Row::new()
                .push(widget::Space::new().width(Length::Fill))
                .push(panel),
        )
        .padding([2, 40, 2, 0])
        .width(Length::Fill)
        .class(cosmic::theme::Container::custom(|theme| {
            let background = theme.cosmic().background(theme.transparent);
            widget::container::Style {
                text_color: Some(background.on.into()),
                icon_color: Some(background.on.into()),
                background: Some(Color::from(background.base).into()),
                ..widget::container::Style::default()
            }
        }));
        let popup = widget::Row::new()
            .push(widget::Space::new().width(Length::Fill))
            .push(popup)
            .padding([0, 24, 0, 0]);
        widget::container(widget::Column::new().spacing(8).push(panel).push(popup))
            .width(Length::Fill)
            .height(Length::Fill)
            .class(cosmic::theme::Container::custom(|_| {
                widget::container::Style {
                    background: Some(backdrop()),
                    ..widget::container::Style::default()
                }
            }))
            .into()
    }
}

/// A dark blue-green gradient, standing in for a wallpaper.
fn backdrop() -> Background {
    Gradient::Linear(
        Linear::new(Radians(2.4))
            .add_stop(0.0, Color::from_rgb8(0x0b, 0x3d, 0x3a))
            .add_stop(1.0, Color::from_rgb8(0x0a, 0x16, 0x24)),
    )
    .into()
}

/// After [`SHOT_DELAY`], saves the window's pixels to `path` and exits.
fn screenshot(id: window::Id, path: String) -> Task<cosmic::Action<Message>> {
    Task::future(tokio::time::sleep(SHOT_DELAY))
        .then(move |()| window::screenshot(id))
        .then(move |shot| {
            let mut file = b"RGBA".to_vec();
            file.extend_from_slice(&shot.size.width.to_le_bytes());
            file.extend_from_slice(&shot.size.height.to_le_bytes());
            file.extend_from_slice(&shot.rgba);
            let code = match std::fs::write(&path, file) {
                Ok(()) => 0,
                Err(why) => {
                    eprintln!("can't write {path}: {why}");
                    1
                }
            };
            // Not `iced::exit`: tearing the window down sometimes crashes, after the file is
            // written but with an exit status the script would take for a failure.
            std::process::exit(code);
            #[allow(unreachable_code)]
            Task::none()
        })
}
