// SPDX-License-Identifier: MPL-2.0

//! Debug builds only: `CODEXBAR_PREVIEW=tests/fixtures/<file>.json cargo run` shows the
//! `Connected` popup for that snapshot in an ordinary window, to check the layout without the
//! panel or the desktop app. `CODEXBAR_PREVIEW_PROVIDER=codex` selects that tab first. The header
//! buttons do nothing.

use cosmic::iced::{Border, Color, Length, Size};
use cosmic::prelude::*;
use cosmic::widget;

use crate::app::Message;
use crate::popup;
use crate::snapshot::{self, Snapshot};

/// As wide as the panel's popup.
const WIDTH: f32 = 360.0;

pub fn run(path: &str) -> cosmic::iced::Result {
    let bytes = std::fs::read(path).unwrap_or_else(|why| panic!("can't read {path}: {why}"));
    let snapshot =
        snapshot::parse(&bytes).unwrap_or_else(|why| panic!("can't parse {path}: {why:?}"));
    let settings = cosmic::app::Settings::default().size(Size::new(WIDTH + 40.0, 900.0));
    cosmic::app::run::<Preview>(settings, snapshot)
}

struct Preview {
    core: cosmic::Core,
    snapshot: Snapshot,
    provider: Option<String>,
}

impl cosmic::Application for Preview {
    type Executor = cosmic::executor::Default;
    type Flags = Snapshot;
    type Message = Message;

    const APP_ID: &'static str = "io.github.leonardo_panseri.cosmic-ext-applet-codexbar.Preview";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(core: cosmic::Core, snapshot: Snapshot) -> (Self, Task<cosmic::Action<Message>>) {
        let preview = Self {
            core,
            snapshot,
            provider: std::env::var("CODEXBAR_PREVIEW_PROVIDER").ok(),
        };
        (preview, Task::none())
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Message> {
        cosmic::iced::keyboard::listen().filter_map(popup::shortcut)
    }

    fn update(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::SelectProvider(provider) => self.provider = Some(provider),
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
        widget::container(popup).padding(20).into()
    }
}
