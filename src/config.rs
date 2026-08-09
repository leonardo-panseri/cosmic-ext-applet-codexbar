// SPDX-License-Identifier: MPL-2.0

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    pub primary: u8,
    pub secondary: u8,
    pub tertiary: u8,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            primary: 80,
            secondary: 80,
            tertiary: 80,
        }
    }
}

impl Thresholds {
    fn normalize(&mut self) {
        self.primary = self.primary.clamp(1, 99);
        self.secondary = self.secondary.clamp(1, 99);
        self.tertiary = self.tertiary.clamp(1, 99);
    }
}

#[derive(Debug, Default, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ThresholdSettings {
    pub defaults: Thresholds,
    pub providers: BTreeMap<String, Thresholds>,
}

impl ThresholdSettings {
    pub fn for_provider(&self, provider: &str) -> Thresholds {
        self.providers
            .get(provider)
            .copied()
            .unwrap_or(self.defaults)
    }

    fn normalize(&mut self) {
        self.defaults.normalize();
        for thresholds in self.providers.values_mut() {
            thresholds.normalize();
        }
    }
}

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

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum PresentationLayout {
    Compact,
    #[default]
    Balanced,
    Detailed,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum UsageBarFill {
    #[default]
    Solid,
    Segmented,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum ResetDisplay {
    #[default]
    Countdown,
    Clock,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum SessionLabel {
    #[default]
    Project,
    Descriptive,
    DescriptiveProject,
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

#[derive(Debug, Clone, CosmicConfigEntry, Eq, PartialEq, Serialize, Deserialize)]
#[version = 8]
pub struct Config {
    pub icon_style: IconStyle,
    pub refresh_interval: RefreshInterval,
    pub refresh_on_popup_open: bool,
    pub overview_providers: Vec<String>,
    pub terminal_app: String,
    pub session_label: SessionLabel,
    pub check_provider_status: bool,
    pub language_override: String,
    pub launch_at_login: bool,
    pub high_contrast: bool,
    pub presentation_layout: PresentationLayout,
    pub switcher_rows: u8,
    pub usage_bar_fill: UsageBarFill,
    pub warning_markers: bool,
    pub notifications_enabled: bool,
    pub notification_sound: bool,
    pub quota_thresholds: ThresholdSettings,
    pub reset_display: ResetDisplay,
    pub show_credits: bool,
    pub show_cost_summary: bool,
    pub cost_summary_days: u16,
    pub hide_personal_info: bool,
    pub show_storage_usage: bool,
    pub show_changelog_links: bool,
    pub show_debug_settings: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            icon_style: IconStyle::default(),
            refresh_interval: RefreshInterval::default(),
            refresh_on_popup_open: true,
            overview_providers: Vec::new(),
            terminal_app: "cosmic-term".into(),
            session_label: SessionLabel::default(),
            check_provider_status: true,
            language_override: String::new(),
            launch_at_login: false,
            high_contrast: false,
            presentation_layout: PresentationLayout::default(),
            switcher_rows: 3,
            usage_bar_fill: UsageBarFill::default(),
            warning_markers: true,
            notifications_enabled: true,
            notification_sound: true,
            quota_thresholds: ThresholdSettings::default(),
            reset_display: ResetDisplay::default(),
            show_credits: true,
            show_cost_summary: false,
            cost_summary_days: 7,
            hide_personal_info: false,
            show_storage_usage: false,
            show_changelog_links: false,
            show_debug_settings: false,
        }
    }
}

impl Config {
    pub fn normalize(&mut self) {
        self.overview_providers.truncate(6);
        self.switcher_rows = self.switcher_rows.clamp(1, 6);
        self.cost_summary_days = if self.cost_summary_days <= 7 { 7 } else { 30 };
        self.quota_thresholds.normalize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_preferences_round_trip_and_enforce_native_limits() {
        let mut config = Config {
            overview_providers: (0..8).map(|index| format!("provider-{index}")).collect(),
            switcher_rows: 99,
            cost_summary_days: 99,
            ..Default::default()
        };

        config.normalize();
        let round_trip: Config =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();

        assert_eq!(round_trip, config);
        assert_eq!(round_trip.overview_providers.len(), 6);
        assert_eq!(round_trip.switcher_rows, 6);
        assert_eq!(round_trip.cost_summary_days, 30);
    }

    #[test]
    fn provider_quota_thresholds_override_global_defaults() {
        let mut settings = ThresholdSettings::default();
        settings.providers.insert(
            "codex".into(),
            Thresholds {
                primary: 90,
                ..Default::default()
            },
        );

        assert_eq!(settings.for_provider("codex").primary, 90);
        assert_eq!(settings.for_provider("claude").primary, 80);
    }
}
