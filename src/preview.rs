// SPDX-License-Identifier: MPL-2.0

//! Debug builds only: `CODEXBAR_PREVIEW=tests/fixtures/<file>.json cargo run` shows the
//! panel button's content and the `Connected` popup for that snapshot in an ordinary window, to
//! check the layout without the panel or the desktop app. `CODEXBAR_PREVIEW_PROVIDER=codex` selects that tab first, and
//! `CODEXBAR_PREVIEW_PANEL_PAGE=1` opens the panel page. The header
//! buttons do nothing.

use cosmic::iced::{Border, Color, Length, Size};
use cosmic::prelude::*;
use cosmic::widget;

use cosmic::widget::segmented_button::SingleSelectModel;

use crate::app::{self, Message};
use crate::config::PanelStyle;
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
    /// As the applet's config, but only in memory.
    hidden: Vec<String>,
    style: PanelStyle,
    styles: SingleSelectModel,
    panel_page: bool,
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
            hidden: Vec::new(),
            style: PanelStyle::default(),
            styles: popup::style_model(PanelStyle::default()),
            panel_page: std::env::var_os("CODEXBAR_PREVIEW_PANEL_PAGE").is_some(),
        };
        (preview, Task::none())
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
        widget::Column::new()
            .spacing(12)
            .padding(20)
            .push(panel)
            .push(popup)
            .into()
    }
}
