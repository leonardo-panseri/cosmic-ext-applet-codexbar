// SPDX-License-Identifier: MPL-2.0

use crate::{
    app::Message,
    codexbar::{CostModel, CostReport},
    fl,
};
use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget::{
    self,
    segmented_button::{Entity, SingleSelectModel},
    settings,
};
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Dashboard {
    pub currencies: Vec<CurrencySpend>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct CurrencySpend {
    pub currency: String,
    pub total: f64,
    pub providers: Vec<ProviderSpend>,
    pub daily: Vec<DailySpend>,
    pub models: Vec<ModelSpend>,
    pub covered_days: usize,
    pub requested_days: u16,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ProviderSpend {
    pub provider: String,
    pub total: f64,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct DailySpend {
    pub date: String,
    pub total: f64,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ModelSpend {
    pub model_name: String,
    pub cost: f64,
    pub total_tokens: u64,
}

impl Dashboard {
    pub fn group(reports: Vec<CostReport>, requested_days: u16) -> Self {
        let mut currencies: BTreeMap<String, CurrencySpend> = BTreeMap::new();
        for report in reports {
            let use_daily_models = report.model_breakdowns.is_empty();
            let group = currencies
                .entry(report.currency_code.clone())
                .or_insert_with(|| CurrencySpend {
                    currency: report.currency_code,
                    requested_days,
                    ..Default::default()
                });
            let provider_total = if report.totals.total_cost == 0.0 {
                report.daily.iter().map(|day| day.total_cost).sum()
            } else {
                report.totals.total_cost
            };
            group.total += provider_total;
            if let Some(provider) = group
                .providers
                .iter_mut()
                .find(|provider| provider.provider == report.provider)
            {
                provider.total += provider_total;
            } else {
                group.providers.push(ProviderSpend {
                    provider: report.provider,
                    total: provider_total,
                });
            }
            if !use_daily_models {
                for model in report.model_breakdowns {
                    add_model(&mut group.models, model);
                }
            }
            for day in report.daily {
                if let Some(existing) = group.daily.iter_mut().find(|item| item.date == day.date) {
                    existing.total += day.total_cost;
                } else {
                    group.daily.push(DailySpend {
                        date: day.date,
                        total: day.total_cost,
                    });
                }
                if use_daily_models {
                    for model in day.model_breakdowns {
                        add_model(&mut group.models, model);
                    }
                }
            }
        }
        for group in currencies.values_mut() {
            group.providers.sort_by(|left, right| {
                right
                    .total
                    .total_cmp(&left.total)
                    .then_with(|| left.provider.cmp(&right.provider))
            });
            group
                .daily
                .sort_by(|left, right| left.date.cmp(&right.date));
            group.models.sort_by(|left, right| {
                right
                    .cost
                    .total_cmp(&left.cost)
                    .then_with(|| left.model_name.cmp(&right.model_name))
            });
            group.covered_days = group.daily.len().min(usize::from(requested_days));
        }
        Self {
            currencies: currencies.into_values().collect(),
        }
    }
}

fn add_model(models: &mut Vec<ModelSpend>, model: CostModel) {
    if let Some(existing) = models
        .iter_mut()
        .find(|item| item.model_name == model.model_name)
    {
        existing.cost += model.cost;
        existing.total_tokens += model.total_tokens;
    } else {
        models.push(ModelSpend {
            model_name: model.model_name,
            cost: model.cost,
            total_tokens: model.total_tokens,
        });
    }
}

pub struct State {
    pub days: u16,
    pub ranges: SingleSelectModel,
    pub dashboard: Dashboard,
    pub refreshing: bool,
    pub error: Option<String>,
    pub failure_count: u32,
    refresh_day: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        let mut ranges = SingleSelectModel::default();
        ranges.insert().text("7 days").data(7_u16).activate();
        ranges.insert().text("30 days").data(30_u16);
        Self {
            days: 7,
            ranges,
            dashboard: Dashboard::default(),
            refreshing: false,
            error: None,
            failure_count: 0,
            refresh_day: None,
        }
    }
}

impl State {
    pub fn set_days(&mut self, days: u16) {
        self.days = if days <= 7 { 7 } else { 30 };
        self.ranges
            .activate_position(if self.days == 7 { 0 } else { 1 });
    }

    pub fn select_range(&mut self, id: Entity) -> bool {
        let Some(days) = self.ranges.data::<u16>(id).copied() else {
            return false;
        };
        self.ranges.activate(id);
        if self.days == days {
            return false;
        }
        self.days = days;
        true
    }

    pub fn request_refresh(&mut self, day: &str) -> bool {
        if self.refreshing {
            return false;
        }
        self.refreshing = true;
        self.refresh_day = Some(day.to_owned());
        true
    }

    pub fn refresh_for_day(&mut self, day: &str) -> bool {
        self.refresh_day.as_deref() != Some(day) && self.request_refresh(day)
    }

    pub fn finish(&mut self, days: u16, result: Result<Vec<CostReport>, String>) {
        self.refreshing = false;
        match result {
            Ok(reports) => {
                self.dashboard = Dashboard::group(reports, days);
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error);
                self.failure_count = self.failure_count.saturating_add(1);
            }
        }
    }
}

pub fn current_day() -> String {
    jiff::Zoned::now().date().to_string()
}

pub fn view(state: &State) -> Element<'_, Message> {
    let refresh: Element<'_, Message> = if state.refreshing {
        widget::indeterminate_circular().size(20.0).into()
    } else {
        widget::button::text(fl!("spend-refresh"))
            .on_press(Message::RefreshSpend)
            .into()
    };
    let mut content = widget::column::with_capacity(state.dashboard.currencies.len() + 5)
        .push(widget::text::title3(fl!("settings-usage-spend")))
        .push(
            widget::row::with_capacity(2)
                .push(
                    widget::segmented_control::horizontal(&state.ranges)
                        .on_activate(Message::SelectSpendRange),
                )
                .push(refresh)
                .spacing(12),
        )
        .push_maybe(
            state
                .error
                .as_ref()
                .map(|error| widget::text::body(format!("{}: {error}", fl!("spend-stale")))),
        )
        .spacing(16);

    if state.dashboard.currencies.is_empty() && !state.refreshing {
        content = content.push(widget::text::body(fl!("spend-no-data")));
    }
    for currency in &state.dashboard.currencies {
        content = content.push(currency_view(currency));
    }
    content = content.push(widget::text::caption(format!(
        "{}: {}",
        fl!("spend-refresh-failures"),
        state.failure_count
    )));
    settings::view_column(vec![content.into()]).into()
}

fn currency_view(currency: &CurrencySpend) -> Element<'_, Message> {
    let code = if currency.currency.is_empty() {
        fl!("spend-unknown-currency")
    } else {
        currency.currency.clone()
    };
    let mut content = widget::column::with_capacity(8)
        .push(
            widget::row::with_capacity(2)
                .push(widget::text::heading(code.clone()))
                .push(widget::text::heading(money(&code, currency.total)))
                .spacing(12),
        )
        .push(widget::text::caption(format!(
            "{} / {} {}",
            currency.covered_days,
            currency.requested_days,
            fl!("spend-days-covered")
        )))
        .push(widget::text::heading(fl!("spend-provider-ranking")))
        .spacing(8);

    for (index, provider) in currency.providers.iter().enumerate() {
        content = content.push(
            widget::row::with_capacity(2)
                .push(widget::text::body(format!(
                    "{}. {}",
                    index + 1,
                    provider.provider
                )))
                .push(widget::space().width(Length::Fill))
                .push(widget::text::body(money(&code, provider.total)))
                .spacing(8),
        );
    }

    content = content.push(widget::text::heading(fl!("spend-daily")));
    let maximum = currency
        .daily
        .iter()
        .map(|day| day.total)
        .fold(0.0_f64, f64::max);
    for day in &currency.daily {
        let units = bar_units(day.total, maximum);
        let bar: Element<'_, Message> = if units == 0 {
            widget::space().width(Length::Fill).into()
        } else {
            widget::row::with_capacity(2)
                .push(
                    widget::container(widget::text(""))
                        .class(cosmic::theme::Container::Card)
                        .width(Length::FillPortion(units))
                        .height(Length::Fixed(12.0)),
                )
                .push(widget::space().width(Length::FillPortion(1000_u16.saturating_sub(units))))
                .into()
        };
        content = content.push(
            widget::row::with_capacity(3)
                .push(widget::text::caption(&day.date).width(Length::Fixed(96.0)))
                .push(widget::container(bar).width(Length::Fill))
                .push(widget::text::caption(money(&code, day.total)))
                .align_y(cosmic::iced::Alignment::Center)
                .spacing(8),
        );
    }

    content = content.push(widget::text::heading(fl!("spend-models")));
    for model in &currency.models {
        content = content.push(
            widget::row::with_capacity(3)
                .push(widget::text::body(&model.model_name))
                .push(widget::space().width(Length::Fill))
                .push(widget::text::caption(format!(
                    "{} · {} {}",
                    money(&code, model.cost),
                    model.total_tokens,
                    fl!("spend-tokens")
                )))
                .spacing(8),
        );
    }

    widget::container(content)
        .class(cosmic::theme::Container::Card)
        .padding(16)
        .width(Length::Fill)
        .into()
}

fn money(currency: &str, amount: f64) -> String {
    format!("{currency} {amount:.2}")
}

fn bar_units(value: f64, maximum: f64) -> u16 {
    if value <= 0.0 || maximum <= 0.0 {
        0
    } else {
        ((value / maximum * 1000.0).round() as u16).clamp(1, 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codexbar::{CostDaily, CostModel, CostReport, CostTotals};

    fn report(provider: &str, currency: &str, days: &[(&str, f64)]) -> CostReport {
        CostReport {
            provider: provider.into(),
            currency_code: currency.into(),
            history_days: 7,
            daily: days
                .iter()
                .map(|(date, cost)| CostDaily {
                    date: (*date).into(),
                    total_cost: *cost,
                    model_breakdowns: vec![CostModel {
                        model_name: format!("{provider}-model"),
                        cost: *cost,
                        total_tokens: 10,
                    }],
                })
                .collect(),
            totals: CostTotals {
                total_cost: days.iter().map(|(_, cost)| cost).sum(),
            },
            ..Default::default()
        }
    }

    #[test]
    fn groups_native_currencies_without_cross_currency_totals() {
        let dashboard = Dashboard::group(
            vec![
                report("codex", "USD", &[("2026-08-08", 2.0), ("2026-08-09", 3.0)]),
                report("claude", "USD", &[("2026-08-09", 7.0)]),
                report("vertex", "EUR", &[("2026-08-09", 5.0)]),
            ],
            7,
        );

        assert_eq!(dashboard.currencies.len(), 2);
        let usd = dashboard
            .currencies
            .iter()
            .find(|group| group.currency == "USD")
            .unwrap();
        assert_eq!(usd.total, 12.0);
        assert_eq!(usd.providers[0].provider, "claude");
        assert_eq!(usd.daily.len(), 2);
        assert_eq!(usd.covered_days, 2);
        assert_eq!(usd.requested_days, 7);
        assert_eq!(usd.models[0].model_name, "claude-model");
        assert_eq!(
            dashboard
                .currencies
                .iter()
                .find(|group| group.currency == "EUR")
                .unwrap()
                .total,
            5.0
        );
    }

    #[test]
    fn prefers_report_model_aggregates_with_daily_fallback() {
        let mut aggregate = report("codex", "USD", &[("2026-08-09", 3.0)]);
        aggregate.model_breakdowns = vec![CostModel {
            model_name: "aggregate-model".into(),
            cost: 3.0,
            total_tokens: 99,
        }];

        let dashboard = Dashboard::group(vec![aggregate], 7);

        assert_eq!(dashboard.currencies[0].models.len(), 1);
        assert_eq!(
            dashboard.currencies[0].models[0].model_name,
            "aggregate-model"
        );
        assert_eq!(dashboard.currencies[0].models[0].total_tokens, 99);
    }

    #[test]
    fn state_keeps_last_good_and_refreshes_once_when_the_day_changes() {
        let mut state = State::default();
        assert!(state.request_refresh("2026-08-09"));
        state.finish(7, Ok(vec![report("codex", "USD", &[("2026-08-09", 3.0)])]));
        assert_eq!(state.dashboard.currencies[0].total, 3.0);

        assert!(!state.refresh_for_day("2026-08-09"));
        assert!(state.refresh_for_day("2026-08-10"));
        assert!(!state.refresh_for_day("2026-08-10"));
        state.finish(7, Err("offline".into()));

        assert_eq!(state.dashboard.currencies[0].total, 3.0);
        assert_eq!(state.error.as_deref(), Some("offline"));
        assert_eq!(state.failure_count, 1);
    }
}
