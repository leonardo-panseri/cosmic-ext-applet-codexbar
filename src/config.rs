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

#[derive(Debug, Default, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 2]
pub struct Config {
    pub icon_style: IconStyle,
}
