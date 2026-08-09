// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{process::Command, time::timeout};

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

    serde_json::from_slice(&output.stdout)
        .map_err(|error| Error(format!("invalid codexbar JSON: {error}")))
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
