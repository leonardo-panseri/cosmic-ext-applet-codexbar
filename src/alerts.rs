// SPDX-License-Identifier: MPL-2.0

use crate::codexbar::{Snapshot, Window};
use crate::config::ThresholdSettings;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const MAX_RULES: usize = 32;
const MAX_ARGS: usize = 32;
const MAX_STRING_BYTES: usize = 4096;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
const COALESCE_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    QuotaLow,
    QuotaReached,
    QuotaReset,
    ProviderUnavailable,
    ProviderRecovered,
    RefreshFailed,
    RefreshRecovered,
    PaceWarning,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QuotaLow => "quota_low",
            Self::QuotaReached => "quota_reached",
            Self::QuotaReset => "quota_reset",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ProviderRecovered => "provider_recovered",
            Self::RefreshFailed => "refresh_failed",
            Self::RefreshRecovered => "refresh_recovered",
            Self::PaceWarning => "pace_warning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    #[serde(rename = "event")]
    pub kind: EventKind,
    pub provider: String,
    pub window: Option<String>,
    #[serde(rename = "usagePercent")]
    pub used_percent: Option<f32>,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub threshold: Option<u8>,
    pub resets_at: Option<String>,
    pub status: Option<String>,
    pub message: Option<String>,
    pub account: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    snapshots: Vec<Snapshot>,
    refresh_error: Option<String>,
}

#[derive(Debug, Default)]
pub struct Coalescer {
    last: HashMap<(EventKind, String, Option<String>, Option<String>), Duration>,
}

impl Coalescer {
    pub fn allow(&mut self, event: &Event, now: Duration) -> bool {
        let key = (
            event.kind,
            event.provider.clone(),
            event.account.clone(),
            event.window.clone(),
        );
        if self
            .last
            .get(&key)
            .is_some_and(|last| now.saturating_sub(*last) < COALESCE_WINDOW)
        {
            return false;
        }
        self.last.insert(key, now);
        true
    }
}

pub fn eligible_rules<'a>(
    rules: &'a [crate::codexbar::HookRule],
    event: &Event,
) -> Result<Vec<&'a crate::codexbar::HookRule>, String> {
    if rules.len() > MAX_RULES {
        return Err("hook configuration exceeds 32 rules".into());
    }
    if rules.iter().any(|rule| {
        rule.arguments.len() > MAX_ARGS
            || rule.executable.len() > MAX_STRING_BYTES
            || rule
                .arguments
                .iter()
                .any(|argument| argument.len() > MAX_STRING_BYTES)
            || !Path::new(&rule.executable).is_absolute()
            || rule.id.len() > MAX_STRING_BYTES
            || rule.event.len() > MAX_STRING_BYTES
            || rule
                .provider
                .as_ref()
                .is_some_and(|provider| provider.len() > MAX_STRING_BYTES)
            || !rule.timeout_seconds.is_finite()
            || !(0.0..=300.0).contains(&rule.timeout_seconds)
            || rule.timeout_seconds == 0.0
            || rule
                .threshold
                .is_some_and(|threshold| !(0.0..=1.0).contains(&threshold) || threshold == 0.0)
    }) {
        return Err("hook configuration exceeds safety limits".into());
    }
    Ok(rules
        .iter()
        .filter(|rule| {
            rule.enabled
                && rule.event == event.kind.as_str()
                && rule
                    .provider
                    .as_deref()
                    .is_none_or(|provider| provider == event.provider)
                && rule.threshold.is_none_or(|threshold| {
                    event
                        .used_percent
                        .is_some_and(|used| f64::from(used) / 100.0 >= threshold)
                })
        })
        .collect())
}

pub async fn deliver(
    notifications: Vec<Event>,
    hook_events: Vec<Event>,
    hooks: crate::codexbar::HooksConfig,
    sound: bool,
    hide_personal_info: bool,
) -> Vec<String> {
    let mut errors = Vec::new();
    for event in &notifications {
        if let Err(error) = notify(event, sound).await {
            errors.push(error);
        }
    }
    if hooks.enabled {
        for event in hook_events {
            let mut event = redact_hook_event(&event, hide_personal_info);
            event
                .timestamp
                .get_or_insert_with(|| jiff::Timestamp::now().to_string());
            if let Err(error) = run_hooks(&event, &hooks.events).await {
                errors.push(error);
            }
        }
    }
    errors
}

async fn notify(event: &Event, sound: bool) -> Result<(), String> {
    let connection = zbus::Connection::session()
        .await
        .map_err(|error| format!("notification bus unavailable: {error}"))?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await
    .map_err(|error| format!("notification service unavailable: {error}"))?;
    let mut hints = HashMap::<&str, zbus::zvariant::Value<'_>>::new();
    if sound {
        hints.insert("sound-name", "message-new-instant".into());
    } else {
        hints.insert("suppress-sound", true.into());
    }
    let summary = notification_summary(event);
    let body = notification_body(event);
    let _: u32 = proxy
        .call(
            "Notify",
            &(
                "CodexBar",
                0_u32,
                "",
                summary,
                body,
                Vec::<String>::new(),
                hints,
                -1_i32,
            ),
        )
        .await
        .map_err(|error| format!("failed to send notification: {error}"))?;
    Ok(())
}

async fn run_hooks(event: &Event, rules: &[crate::codexbar::HookRule]) -> Result<(), String> {
    let rules = eligible_rules(rules, event)?;
    let payload = hook_payload(event, false)?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err("hook payload exceeds 64 KiB".into());
    }
    let mut errors = Vec::new();
    for rule in rules {
        if let Err(error) = run_hook(event, rule, &payload).await {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn redact_hook_event(event: &Event, hide_personal_info: bool) -> Event {
    let mut event = event.clone();
    if hide_personal_info {
        event.account = None;
    }
    event
}

fn hook_payload(event: &Event, hide_personal_info: bool) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&redact_hook_event(event, hide_personal_info))
        .map_err(|error| error.to_string())
}

async fn run_hook(
    event: &Event,
    rule: &crate::codexbar::HookRule,
    payload: &[u8],
) -> Result<(), String> {
    let mut command = tokio::process::Command::new(&rule.executable);
    command
        .args(&rule.arguments)
        .env("CODEXBAR_EVENT", event.kind.as_str())
        .env("CODEXBAR_PROVIDER", &event.provider)
        .env("CODEXBAR_WINDOW", event.window.as_deref().unwrap_or(""))
        .env("CODEXBAR_ACCOUNT", event.account.as_deref().unwrap_or(""))
        .env(
            "CODEXBAR_USAGE_PERCENT",
            event
                .used_percent
                .map(|value| value.to_string())
                .unwrap_or_default(),
        )
        .env(
            "CODEXBAR_USED",
            event
                .used
                .map(|value| value.to_string())
                .unwrap_or_default(),
        )
        .env(
            "CODEXBAR_LIMIT",
            event
                .limit
                .map(|value| value.to_string())
                .unwrap_or_default(),
        )
        .env(
            "CODEXBAR_RESET_AT",
            event.resets_at.as_deref().unwrap_or(""),
        )
        .env("CODEXBAR_STATUS", event.status.as_deref().unwrap_or(""))
        .env(
            "CODEXBAR_TIMESTAMP",
            event.timestamp.as_deref().unwrap_or(""),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to run hook {}: {error}", rule.id))?;
    let timeout = Duration::from_secs_f64(rule.timeout_seconds);
    let result = async {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(payload)
                .await
                .map_err(|error| format!("failed to write hook {} payload: {error}", rule.id))?;
        }
        child
            .wait()
            .await
            .map_err(|error| format!("hook {} failed: {error}", rule.id))
    };
    match tokio::time::timeout(timeout, result).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!("hook {} exited with {status}", rule.id)),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(format!("hook {} timed out", rule.id)),
    }
}

fn notification_summary(event: &Event) -> String {
    match event.kind {
        EventKind::QuotaLow => "CodexBar quota warning",
        EventKind::QuotaReached => "CodexBar quota reached",
        EventKind::QuotaReset => "CodexBar quota reset",
        EventKind::ProviderUnavailable => "CodexBar provider unavailable",
        EventKind::ProviderRecovered => "CodexBar provider recovered",
        EventKind::RefreshFailed => "CodexBar refresh failed",
        EventKind::RefreshRecovered => "CodexBar refresh recovered",
        EventKind::PaceWarning => "CodexBar pace warning",
    }
    .into()
}

fn notification_body(event: &Event) -> String {
    let window = event
        .window
        .as_deref()
        .map(|window| format!(" {window}"))
        .unwrap_or_default();
    let usage = event
        .used_percent
        .map(|used| format!(" at {used:.0}%"))
        .unwrap_or_default();
    event
        .message
        .clone()
        .unwrap_or_else(|| format!("{}{window}{usage}", event.provider))
}

impl State {
    pub fn loaded(snapshots: Vec<Snapshot>) -> Self {
        Self {
            snapshots,
            refresh_error: None,
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            snapshots: Vec::new(),
            refresh_error: Some(error.into()),
        }
    }
}

pub fn diff(
    previous: Option<&State>,
    current: &State,
    threshold_settings: &ThresholdSettings,
) -> Vec<Event> {
    let Some(previous) = previous else {
        return Vec::new();
    };
    let mut events = Vec::new();
    match (&previous.refresh_error, &current.refresh_error) {
        (None, Some(error)) => events.push(Event {
            kind: EventKind::RefreshFailed,
            provider: "all".into(),
            window: None,
            used_percent: None,
            used: None,
            limit: None,
            threshold: None,
            resets_at: None,
            status: None,
            message: Some(error.clone()),
            account: None,
            timestamp: None,
        }),
        (Some(_), None) => events.push(Event {
            kind: EventKind::RefreshRecovered,
            provider: "all".into(),
            window: None,
            used_percent: None,
            used: None,
            limit: None,
            threshold: None,
            resets_at: None,
            status: None,
            message: None,
            account: None,
            timestamp: None,
        }),
        _ => {}
    }
    for current_snapshot in &current.snapshots {
        let Some(previous_snapshot) = previous
            .snapshots
            .iter()
            .find(|snapshot| snapshot.provider == current_snapshot.provider)
        else {
            continue;
        };
        let previous = windows(previous_snapshot);
        let current_windows = windows(current_snapshot);
        let thresholds = threshold_settings.for_provider(&current_snapshot.provider);
        for index in 0..3 {
            let (Some(previous), Some(current)) = (previous[index], current_windows[index]) else {
                continue;
            };
            let threshold = [
                thresholds.primary,
                thresholds.secondary,
                thresholds.tertiary,
            ][index];
            if previous.used_percent < f32::from(threshold)
                && current.used_percent >= f32::from(threshold)
            {
                events.push(Event {
                    kind: EventKind::QuotaLow,
                    provider: current_snapshot.provider.clone(),
                    window: Some(["primary", "secondary", "tertiary"][index].into()),
                    used_percent: Some(current.used_percent),
                    used: current.used,
                    limit: current.limit,
                    threshold: Some(threshold),
                    resets_at: current.resets_at.clone(),
                    status: None,
                    message: None,
                    account: account(current_snapshot),
                    timestamp: timestamp(current_snapshot),
                });
            }
            if index == 0 && previous.used_percent < 100.0 && current.used_percent >= 100.0 {
                events.push(Event {
                    kind: EventKind::QuotaReached,
                    provider: current_snapshot.provider.clone(),
                    window: Some(["primary", "secondary", "tertiary"][index].into()),
                    used_percent: Some(current.used_percent),
                    used: current.used,
                    limit: current.limit,
                    threshold: None,
                    resets_at: current.resets_at.clone(),
                    status: None,
                    message: None,
                    account: account(current_snapshot),
                    timestamp: timestamp(current_snapshot),
                });
            }
            if previous.resets_at.is_some()
                && current.resets_at.is_some()
                && previous.resets_at != current.resets_at
            {
                events.push(Event {
                    kind: EventKind::QuotaReset,
                    provider: current_snapshot.provider.clone(),
                    window: Some(["primary", "secondary", "tertiary"][index].into()),
                    used_percent: Some(current.used_percent),
                    used: current.used,
                    limit: current.limit,
                    threshold: None,
                    resets_at: current.resets_at.clone(),
                    status: None,
                    message: None,
                    account: account(current_snapshot),
                    timestamp: timestamp(current_snapshot),
                });
            }
        }
        let previous_pace = pace_windows(previous_snapshot);
        let current_pace = pace_windows(current_snapshot);
        for index in 0..3 {
            let warned = |pace: Option<&crate::codexbar::PaceWindow>| {
                pace.is_some_and(|pace| pace.stage == "farAhead" && !pace.will_last_to_reset)
            };
            if !warned(previous_pace[index]) && warned(current_pace[index]) {
                events.push(Event {
                    kind: EventKind::PaceWarning,
                    provider: current_snapshot.provider.clone(),
                    window: Some(["primary", "secondary", "tertiary"][index].into()),
                    used_percent: current_pace[index].map(|pace| pace.expected_used_percent),
                    used: current_windows[index].and_then(|window| window.used),
                    limit: current_windows[index].and_then(|window| window.limit),
                    threshold: None,
                    resets_at: current_windows[index].and_then(|window| window.resets_at.clone()),
                    status: None,
                    message: current_pace[index].map(|pace| pace.summary.clone()),
                    account: account(current_snapshot),
                    timestamp: timestamp(current_snapshot),
                });
            }
        }
        if let (Some(previous), Some(current)) =
            (&previous_snapshot.status, &current_snapshot.status)
        {
            let previous_ok = operational(&previous.indicator);
            let current_ok = operational(&current.indicator);
            let kind = match (previous_ok, current_ok) {
                (true, false) => Some(EventKind::ProviderUnavailable),
                (false, true) => Some(EventKind::ProviderRecovered),
                _ => None,
            };
            if let Some(kind) = kind {
                events.push(Event {
                    kind,
                    provider: current_snapshot.provider.clone(),
                    window: None,
                    used_percent: None,
                    used: None,
                    limit: None,
                    threshold: None,
                    resets_at: None,
                    status: Some(current.indicator.clone()),
                    message: None,
                    account: account(current_snapshot),
                    timestamp: timestamp(current_snapshot),
                });
            }
        }
        match (&previous_snapshot.error, &current_snapshot.error) {
            (None, Some(error)) => events.push(Event {
                kind: EventKind::RefreshFailed,
                provider: current_snapshot.provider.clone(),
                window: None,
                used_percent: None,
                used: None,
                limit: None,
                threshold: None,
                resets_at: None,
                status: None,
                message: Some(error.message.clone()),
                account: account(current_snapshot),
                timestamp: timestamp(current_snapshot),
            }),
            (Some(_), None) => events.push(Event {
                kind: EventKind::RefreshRecovered,
                provider: current_snapshot.provider.clone(),
                window: None,
                used_percent: None,
                used: None,
                limit: None,
                threshold: None,
                resets_at: None,
                status: None,
                message: None,
                account: account(current_snapshot),
                timestamp: timestamp(current_snapshot),
            }),
            _ => {}
        }
    }
    events
}

fn operational(indicator: &str) -> bool {
    matches!(indicator, "" | "none" | "operational")
}

fn windows(snapshot: &Snapshot) -> [Option<&Window>; 3] {
    let usage = snapshot.usage.as_ref();
    [
        usage.and_then(|usage| usage.primary.as_ref()),
        usage.and_then(|usage| usage.secondary.as_ref()),
        usage.and_then(|usage| usage.tertiary.as_ref()),
    ]
}

fn pace_windows(snapshot: &Snapshot) -> [Option<&crate::codexbar::PaceWindow>; 3] {
    let pace = snapshot.pace.as_ref();
    [
        pace.and_then(|pace| pace.primary.as_ref()),
        pace.and_then(|pace| pace.secondary.as_ref()),
        pace.and_then(|pace| pace.tertiary.as_ref()),
    ]
}

fn account(snapshot: &Snapshot) -> Option<String> {
    snapshot
        .usage
        .as_ref()
        .map(|usage| usage.identity.account_email.clone())
        .filter(|account| !account.is_empty())
}

fn timestamp(snapshot: &Snapshot) -> Option<String> {
    snapshot
        .usage
        .as_ref()
        .map(|usage| usage.updated_at.clone())
        .or_else(|| {
            snapshot
                .status
                .as_ref()
                .map(|status| status.updated_at.clone())
        })
        .filter(|timestamp| !timestamp.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codexbar::{Pace, PaceWindow, Snapshot, Usage, Window};
    use crate::config::{ThresholdSettings, Thresholds};

    fn snapshot(used_percent: f32) -> Snapshot {
        Snapshot {
            provider: "codex".into(),
            usage: Some(Usage {
                primary: Some(Window {
                    used_percent,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn privacy_redacts_hook_payload_without_touching_source_event() {
        let event = Event {
            kind: EventKind::QuotaLow,
            provider: "codex".into(),
            window: Some("primary".into()),
            used_percent: Some(80.0),
            used: None,
            limit: None,
            threshold: Some(80),
            resets_at: None,
            status: None,
            message: None,
            account: Some("dev@example.com".into()),
            timestamp: None,
        };

        let payload = hook_payload(&event, true).unwrap();

        assert!(
            !String::from_utf8(payload)
                .unwrap()
                .contains("dev@example.com")
        );
        assert_eq!(event.account.as_deref(), Some("dev@example.com"));
    }

    #[test]
    fn threshold_crossing_fires_once_and_rearms_below_threshold() {
        let thresholds = ThresholdSettings::default();
        let below = State::loaded(vec![snapshot(79.0)]);
        let above = State::loaded(vec![snapshot(80.0)]);
        let higher = State::loaded(vec![snapshot(90.0)]);

        assert_eq!(
            diff(Some(&below), &above, &thresholds)[0].kind,
            EventKind::QuotaLow
        );
        assert!(diff(Some(&above), &higher, &thresholds).is_empty());
        assert!(diff(Some(&higher), &below, &thresholds).is_empty());
        assert_eq!(
            diff(Some(&below), &above, &thresholds)[0].kind,
            EventKind::QuotaLow
        );
    }

    #[test]
    fn provider_threshold_override_drives_crossing_event() {
        let mut thresholds = ThresholdSettings::default();
        thresholds.providers.insert(
            "codex".into(),
            Thresholds {
                primary: 90,
                ..Default::default()
            },
        );
        let below = State::loaded(vec![snapshot(85.0)]);
        let above = State::loaded(vec![snapshot(90.0)]);

        assert_eq!(
            diff(Some(&below), &above, &thresholds)[0].threshold,
            Some(90)
        );
    }

    #[test]
    fn quota_reached_only_fires_on_the_transition_to_full() {
        let thresholds = ThresholdSettings::default();
        let almost = State::loaded(vec![snapshot(99.0)]);
        let full = State::loaded(vec![snapshot(100.0)]);

        assert!(
            diff(Some(&almost), &full, &thresholds)
                .iter()
                .any(|event| event.kind == EventKind::QuotaReached)
        );
        assert!(
            !diff(Some(&full), &full, &thresholds)
                .iter()
                .any(|event| event.kind == EventKind::QuotaReached)
        );
    }

    #[test]
    fn quota_reached_is_session_only() {
        let secondary = |used_percent| Snapshot {
            provider: "codex".into(),
            usage: Some(Usage {
                secondary: Some(Window {
                    used_percent,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(
            diff(
                Some(&State::loaded(vec![secondary(99.0)])),
                &State::loaded(vec![secondary(100.0)]),
                &ThresholdSettings::default(),
            )
            .iter()
            .all(|event| event.kind != EventKind::QuotaReached)
        );
    }

    #[test]
    fn moving_reset_time_emits_quota_reset() {
        let mut previous = snapshot(50.0);
        previous
            .usage
            .as_mut()
            .unwrap()
            .primary
            .as_mut()
            .unwrap()
            .resets_at = Some("2026-08-10T10:00:00Z".into());
        let mut current = previous.clone();
        current
            .usage
            .as_mut()
            .unwrap()
            .primary
            .as_mut()
            .unwrap()
            .resets_at = Some("2026-08-17T10:00:00Z".into());

        let events = diff(
            Some(&State::loaded(vec![previous])),
            &State::loaded(vec![current]),
            &ThresholdSettings::default(),
        );

        assert_eq!(events[0].kind, EventKind::QuotaReset);
    }

    #[test]
    fn provider_service_transitions_emit_unavailable_and_recovered() {
        let with_status = |indicator: &str| Snapshot {
            provider: "codex".into(),
            status: Some(crate::codexbar::Status {
                indicator: indicator.into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let operational = State::loaded(vec![with_status("none")]);
        let unavailable = State::loaded(vec![with_status("major")]);

        assert_eq!(
            diff(
                Some(&operational),
                &unavailable,
                &ThresholdSettings::default()
            )[0]
            .kind,
            EventKind::ProviderUnavailable
        );
        assert_eq!(
            diff(
                Some(&unavailable),
                &operational,
                &ThresholdSettings::default()
            )[0]
            .kind,
            EventKind::ProviderRecovered
        );
    }

    #[test]
    fn refresh_transitions_emit_failed_and_recovered() {
        let loaded = State::loaded(vec![snapshot(20.0)]);
        let failed = State::failed("network unavailable");

        assert_eq!(
            diff(Some(&loaded), &failed, &ThresholdSettings::default())[0].kind,
            EventKind::RefreshFailed
        );
        assert_eq!(
            diff(Some(&failed), &loaded, &ThresholdSettings::default())[0].kind,
            EventKind::RefreshRecovered
        );
    }

    #[test]
    fn predictive_pace_warning_is_edge_triggered() {
        let with_pace = |stage: &str, will_last_to_reset| Snapshot {
            provider: "codex".into(),
            pace: Some(Pace {
                primary: Some(PaceWindow {
                    stage: stage.into(),
                    will_last_to_reset,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let safe = State::loaded(vec![with_pace("onTrack", true)]);
        let warning = State::loaded(vec![with_pace("farAhead", false)]);

        assert_eq!(
            diff(Some(&safe), &warning, &ThresholdSettings::default())[0].kind,
            EventKind::PaceWarning
        );
        assert!(diff(Some(&warning), &warning, &ThresholdSettings::default()).is_empty());
    }

    #[test]
    fn hook_rule_and_argument_caps_fail_closed() {
        let rule = || crate::codexbar::HookRule {
            id: "notify".into(),
            enabled: true,
            event: "quota_low".into(),
            executable: "/usr/bin/true".into(),
            ..Default::default()
        };
        assert!(eligible_rules(&vec![rule(); 33], &event(EventKind::QuotaLow)).is_err());

        let mut too_many_args = rule();
        too_many_args.arguments = vec!["argument".into(); 33];
        assert!(eligible_rules(&[too_many_args], &event(EventKind::QuotaLow)).is_err());
    }

    #[test]
    fn hook_json_uses_the_codexbar_payload_contract() {
        let mut event = event(EventKind::QuotaLow);
        event.used = Some(8.0);
        event.limit = Some(10.0);
        event.resets_at = Some("2026-08-10T10:00:00Z".into());
        let payload = serde_json::to_value(event).unwrap();

        assert_eq!(payload["event"], "quota_low");
        assert_eq!(payload["usagePercent"], 80.0);
        assert_eq!(payload["used"], 8.0);
        assert_eq!(payload["limit"], 10.0);
        assert_eq!(payload["resetsAt"], "2026-08-10T10:00:00Z");
        assert!(payload.get("kind").is_none());
    }

    #[test]
    fn identical_hook_events_are_coalesced_for_ten_minutes() {
        let mut coalescer = Coalescer::default();
        let event = event(EventKind::RefreshFailed);

        assert!(coalescer.allow(&event, std::time::Duration::ZERO));
        assert!(!coalescer.allow(&event, std::time::Duration::from_secs(599)));
        assert!(coalescer.allow(&event, std::time::Duration::from_secs(600)));
    }

    fn event(kind: EventKind) -> Event {
        Event {
            kind,
            provider: "codex".into(),
            window: Some("primary".into()),
            used_percent: Some(80.0),
            used: None,
            limit: None,
            threshold: Some(80),
            resets_at: None,
            status: None,
            message: None,
            account: None,
            timestamp: None,
        }
    }
}
