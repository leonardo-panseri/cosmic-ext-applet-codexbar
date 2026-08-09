// SPDX-License-Identifier: MPL-2.0

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum IconStyle {
    Only,
    #[default]
    Percent,
    Label,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum RefreshInterval {
    #[default]
    Adaptive,
    OneMinute,
    TwoMinutes,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
}

impl RefreshInterval {
    pub fn fixed_minutes(self) -> Option<u64> {
        match self {
            Self::Adaptive => None,
            Self::OneMinute => Some(1),
            Self::TwoMinutes => Some(2),
            Self::FiveMinutes => Some(5),
            Self::FifteenMinutes => Some(15),
            Self::ThirtyMinutes => Some(30),
        }
    }
}

#[derive(Debug, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 3]
pub struct Config {
    pub icon_style: IconStyle,
    pub refresh_interval: RefreshInterval,
    pub refresh_on_popup_open: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            icon_style: IconStyle::default(),
            refresh_interval: RefreshInterval::default(),
            refresh_on_popup_open: true,
        }
    }
}
