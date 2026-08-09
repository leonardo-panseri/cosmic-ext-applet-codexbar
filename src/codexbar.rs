// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{process::Command, time::timeout};

use crate::sessions::AgentSession;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

pub type Timestamp = String;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Provider {
    #[serde(rename = "provider")]
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub default_enabled: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConfigDump {
    pub providers: Vec<ProviderConfig>,
    pub version: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    #[serde(flatten)]
    pub fields: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HooksConfig {
    pub enabled: bool,
    #[serde(alias = "rules")]
    pub events: Vec<HookRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HookRule {
    pub id: String,
    pub enabled: bool,
    pub event: String,
    pub provider: Option<String>,
    pub threshold: Option<f64>,
    pub executable: String,
    pub arguments: Vec<String>,
    pub timeout_seconds: f64,
}

impl Default for HookRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: false,
            event: String::new(),
            provider: None,
            threshold: None,
            executable: String::new(),
            arguments: Vec::new(),
            timeout_seconds: 10.0,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Snapshot {
    pub provider: String,
    pub source: String,
    pub usage: Option<Usage>,
    pub pace: Option<Pace>,
    pub credits: Option<Credits>,
    pub status: Option<Status>,
    pub error: Option<ProviderError>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Usage {
    pub identity: Identity,
    pub primary: Option<Window>,
    pub secondary: Option<Window>,
    pub tertiary: Option<Window>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Identity {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    pub login_method: String,
    pub account_email: String,
    pub account_organization: String,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Window {
    pub used_percent: f32,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub resets_at: Option<Timestamp>,
    pub reset_description: Option<String>,
    pub window_minutes: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Pace {
    pub primary: Option<PaceWindow>,
    pub secondary: Option<PaceWindow>,
    pub tertiary: Option<PaceWindow>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PaceWindow {
    pub stage: String,
    pub delta_percent: f32,
    pub summary: String,
    pub expected_used_percent: f32,
    pub will_last_to_reset: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Credits {
    pub remaining: f64,
    pub updated_at: Timestamp,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Status {
    pub indicator: String,
    pub description: String,
    pub url: String,
    pub updated_at: Timestamp,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderError {
    pub code: i32,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CostReport {
    pub provider: String,
    pub currency_code: String,
    pub history_days: u16,
    pub daily: Vec<CostDaily>,
    pub model_breakdowns: Vec<CostModel>,
    pub totals: CostTotals,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CostDaily {
    pub date: String,
    pub total_cost: f64,
    pub model_breakdowns: Vec<CostModel>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CostModel {
    pub model_name: String,
    pub cost: f64,
    pub total_tokens: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CostTotals {
    pub total_cost: f64,
}

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub async fn usage_snapshot(check_status: bool) -> Result<Vec<Snapshot>, Error> {
    run_json(&usage_args(check_status)).await
}

pub async fn cost(days: u16) -> Result<Vec<CostReport>, Error> {
    run_json(&[
        "cost".to_owned(),
        "--days".to_owned(),
        days.to_string(),
        "--json".to_owned(),
    ])
    .await
}

pub async fn sessions() -> Result<Vec<AgentSession>, Error> {
    run_json(&["sessions", "--json"]).await
}

fn usage_args(check_status: bool) -> Vec<&'static str> {
    let mut args = vec!["usage", "--provider", "all", "--format", "json"];
    if check_status {
        args.push("--status");
    }
    args
}

pub async fn providers() -> Result<Vec<Provider>, Error> {
    run_json(&["config", "providers", "--json"]).await
}

pub async fn config_dump() -> Result<ConfigDump, Error> {
    run_json(&["config", "dump", "--json"]).await
}

pub async fn hooks() -> Result<HooksConfig, Error> {
    run_json(&["hooks", "list", "--format", "json"]).await
}

pub async fn set_hooks_enabled(enabled: bool) -> Result<(), Error> {
    run(&["hooks", if enabled { "enable" } else { "disable" }])
        .await
        .map(drop)
}

pub async fn test_hook(event: &str, provider: &str) -> Result<(), Error> {
    run(&["hooks", "test", event, "--provider", provider])
        .await
        .map(drop)
}

fn provider_toggle_args(provider: &str, enabled: bool) -> [String; 5] {
    [
        "config".into(),
        if enabled { "enable" } else { "disable" }.into(),
        "--provider".into(),
        provider.into(),
        "--json".into(),
    ]
}

pub async fn set_provider_enabled(provider: &str, enabled: bool) -> Result<(), Error> {
    let args = provider_toggle_args(provider, enabled);
    run_json::<serde_json::Value, _>(&args).await.map(drop)
}

pub fn sign_in_command(provider: &str) -> Option<(&'static str, &'static [&'static str])> {
    match provider {
        "codex" => Some(("codex", &["login"])),
        "claude" => Some(("claude", &["auth", "login"])),
        "cursor" => Some(("cursor-agent", &["login"])),
        _ => None,
    }
}

pub async fn launch_sign_in(provider: &str) -> Result<(), Error> {
    let Some((program, args)) = sign_in_command(provider) else {
        return Err(Error(format!("sign in is not supported for {provider}")));
    };
    Command::new(program)
        .args(args)
        .spawn()
        .map(drop)
        .map_err(|error| Error(format!("failed to launch {program}: {error}")))
}

async fn run_json<T, S>(args: &[S]) -> Result<T, Error>
where
    T: DeserializeOwned,
    S: AsRef<OsStr>,
{
    let output = run(args).await?;
    serde_json::from_slice(&output)
        .map_err(|error| Error(format!("invalid codexbar JSON: {error}")))
}

async fn run<S>(args: &[S]) -> Result<Vec<u8>, Error>
where
    S: AsRef<OsStr>,
{
    let mut command = Command::new("codexbar");
    command.args(args).kill_on_drop(true);

    let output = match timeout(COMMAND_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return Err(Error(format!("failed to run codexbar: {error}"))),
        Err(_) => return Err(Error("codexbar timed out after 30 seconds".into())),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if stderr.is_empty() {
            output.status.to_string()
        } else {
            stderr
        };
        return Err(Error(format!("codexbar failed: {detail}")));
    }

    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_configuration_keeps_cli_order() {
        let providers: Vec<Provider> = serde_json::from_str(
            r#"[
                {"provider":"claude","displayName":"Claude","enabled":true},
                {"provider":"codex","displayName":"Codex","enabled":true}
            ]"#,
        )
        .unwrap();

        assert_eq!(
            providers
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            ["claude", "codex"]
        );
    }

    #[test]
    fn provider_toggle_uses_codexbar_config_command() {
        let enable = provider_toggle_args("claude", true);
        assert_eq!(
            enable.each_ref().map(String::as_str),
            ["config", "enable", "--provider", "claude", "--json"]
        );
        let disable = provider_toggle_args("claude", false);
        assert_eq!(
            disable.each_ref().map(String::as_str),
            ["config", "disable", "--provider", "claude", "--json"]
        );
    }

    #[test]
    fn config_dump_preserves_generic_provider_fields() {
        let dump: ConfigDump = serde_json::from_str(
            r#"{"providers":[{"id":"alibabatokenplan","enabled":false,"region":"intl"}],"version":1}"#,
        )
        .unwrap();

        assert_eq!(dump.providers[0].id, "alibabatokenplan");
        assert_eq!(dump.providers[0].fields["region"], "intl");
    }

    #[test]
    fn supported_provider_sign_in_uses_provider_cli() {
        assert_eq!(sign_in_command("codex"), Some(("codex", &["login"][..])));
        assert_eq!(
            sign_in_command("claude"),
            Some(("claude", &["auth", "login"][..]))
        );
        assert!(sign_in_command("openrouter").is_none());
    }

    #[test]
    fn provider_status_flag_follows_applet_preference() {
        assert!(usage_args(true).contains(&"--status"));
        assert!(!usage_args(false).contains(&"--status"));
    }

    #[test]
    fn hook_list_uses_existing_codexbar_rule_schema() {
        let hooks: HooksConfig = serde_json::from_str(
            r#"{"enabled":true,"events":[{"id":"warn","enabled":true,"event":"quota_low","provider":"codex","threshold":0.8,"executable":"/usr/bin/notify-send","arguments":["quota"],"timeoutSeconds":5}]}"#,
        )
        .unwrap();

        assert!(hooks.enabled);
        assert_eq!(hooks.events[0].event, "quota_low");
        assert_eq!(hooks.events[0].arguments, ["quota"]);
        assert_eq!(hooks.events[0].timeout_seconds, 5.0);
    }

    #[test]
    fn deserializes_narrow_cost_payload() {
        let reports: Vec<CostReport> = serde_json::from_str(
            r#"[{"provider":"codex","currencyCode":"USD","historyDays":7,"daily":[{"date":"2026-08-09","totalCost":1.25,"modelBreakdowns":[{"modelName":"gpt-5","cost":1.25,"totalTokens":100}]}],"totals":{"totalCost":1.25}}]"#,
        )
        .unwrap();

        assert_eq!(reports[0].currency_code, "USD");
        assert_eq!(reports[0].daily[0].model_breakdowns[0].model_name, "gpt-5");
        assert_eq!(reports[0].totals.total_cost, 1.25);
    }

    #[test]
    fn deserializes_ragged_usage_snapshot() {
        let payload = r#"[
            {
                "provider": "codex",
                "source": "oauth",
                "usage": {
                    "identity": { "providerID": "codex", "accountEmail": "dev@example.com" },
                    "primary": null,
                    "secondary": {
                        "usedPercent": 42.5,
                        "resetsAt": "2026-08-16T09:37:19Z",
                        "windowMinutes": 10080
                    },
                    "updatedAt": "2026-08-09T10:05:23Z"
                }
            },
            {
                "provider": "openai",
                "source": "auto",
                "error": { "code": 1, "kind": "provider", "message": "Not configured" }
            },
            { "provider": "missing-fields" }
        ]"#;

        let snapshots: Vec<Snapshot> = serde_json::from_str(payload).unwrap();
        assert_eq!(snapshots.len(), 3);
        assert!(snapshots[0].usage.as_ref().unwrap().primary.is_none());
        assert!(snapshots[1].usage.is_none());
        assert!(snapshots[2].usage.is_none());

        let round_trip = serde_json::to_value(&snapshots).unwrap();
        assert_eq!(round_trip[0]["usage"]["secondary"]["usedPercent"], 42.5);
    }
}
