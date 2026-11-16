// SPDX-License-Identifier: MPL-2.0

//! Snapshot v1 from `codexbar-linux`, as documented under "Adapter interface" in upstream's
//! `Integrations/Linux/README.md`.
//!
//! Only the fields the applet draws are modelled. Every field has a default and unknown fields
//! are ignored, so the desktop app can add fields without breaking us. Any other `schemaVersion`
//! is [`Incompatible`].

use serde::Deserialize;

/// The only snapshot schema this applet understands.
pub const SCHEMA_VERSION: u64 = 1;

/// Why a reply can't be used: it isn't valid JSON, or its `schemaVersion` isn't
/// [`SCHEMA_VERSION`]. Both mean the desktop app speaks a protocol this applet doesn't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incompatible {
    InvalidJson(String),
    /// `None` when the field is missing or not an unsigned integer.
    SchemaVersion(Option<u64>),
}

impl std::fmt::Display for Incompatible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson(reason) => write!(f, "invalid JSON: {reason}"),
            Self::SchemaVersion(Some(version)) => write!(f, "unsupported schemaVersion {version}"),
            Self::SchemaVersion(None) => f.write_str("missing schemaVersion"),
        }
    }
}

impl std::error::Error for Incompatible {}

/// Parses one snapshot reply, checking `schemaVersion` before anything else.
pub fn parse(json: &[u8]) -> Result<Snapshot, Incompatible> {
    let value: serde_json::Value =
        serde_json::from_slice(json).map_err(|e| Incompatible::InvalidJson(e.to_string()))?;
    let version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64);
    if version != Some(SCHEMA_VERSION) {
        return Err(Incompatible::SchemaVersion(version));
    }
    Snapshot::deserialize(value).map_err(|e| Incompatible::InvalidJson(e.to_string()))
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Snapshot {
    pub summary: String,
    /// `"remaining"` or `"used"`. Window values are already converted to this preference.
    pub quota_display: String,
    pub busy: bool,
    pub stale: bool,
    pub error: String,
    /// Local time of the last update, as `HH:mm`, or empty before the first one.
    pub updated: String,
    /// Missing on older backends; see [`Snapshot::bar`].
    pub bar_entries: Vec<BarEntry>,
    pub entries: Vec<Entry>,
    pub cost_busy: bool,
    pub cost_error: String,
    pub spending: Vec<Spending>,
}

/// What the horizontal panel button shows (plan §4.2), before staleness is applied.
#[derive(Debug, Clone, PartialEq)]
pub enum Bar<'a> {
    /// The bar entries the user hasn't hidden, then `+{more}` when `more > 0`.
    Entries {
        entries: Vec<&'a BarEntry>,
        more: usize,
    },
    /// Older backends have no bar entries, only the preformatted summary.
    Summary(&'a str),
    /// Nothing to show yet, for example in the first seconds after the desktop app starts.
    Empty,
}

/// How many of a provider's windows the panel draws as bars: three fit in an icon's height.
const METERS: usize = 3;

/// One of the small bars next to a provider in the panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meter {
    /// `displayValue`, so the bar fills as the popup's bar does.
    pub value: f64,
    pub warning: bool,
}

impl Snapshot {
    /// The panel's content, without the providers in `hidden`. `more` counts the entries the
    /// desktop app left out of the bar; the hidden ones were left out on purpose and aren't
    /// counted. The summary of older backends can't be filtered.
    pub fn bar(&self, hidden: &[String]) -> Bar<'_> {
        let shown = |provider: &str| !hidden.iter().any(|hidden| hidden == provider);
        if !self.bar_entries.is_empty() {
            let entries: Vec<_> = self
                .bar_entries
                .iter()
                .filter(|bar| shown(&bar.provider))
                .collect();
            if entries.is_empty() {
                return Bar::Empty;
            }
            let more = self
                .entries
                .iter()
                .filter(|entry| shown(&entry.provider))
                .filter(|entry| {
                    !self
                        .bar_entries
                        .iter()
                        .any(|bar| bar.provider == entry.provider)
                })
                .count();
            Bar::Entries { entries, more }
        } else if !self.summary.is_empty() {
            Bar::Summary(&self.summary)
        } else {
            Bar::Empty
        }
    }

    /// The provider's first three windows for the panel's bars, such as 5 hour, 7 day and a
    /// scoped cap like "Fable only". Scoped caps come after the windows that cover the whole
    /// provider, so they're the ones left out when there are more.
    pub fn meters(&self, provider: &str) -> Vec<Meter> {
        let windows = self
            .entries
            .iter()
            .find(|entry| entry.provider == provider)
            .map_or(&[][..], |entry| &entry.windows);
        let general = windows.iter().filter(|window| !window.scoped);
        let scoped = windows.iter().filter(|window| window.scoped);
        general
            .chain(scoped)
            .take(METERS)
            .map(|window| Meter {
                value: window.display_value,
                warning: window.warning,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct BarEntry {
    pub provider: String,
    /// Short text such as `CL`, shown when there's no logo for the provider.
    pub tag: String,
    /// Already formatted, for example `90%` or `5H 90% · 7D 84% · -30%`.
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Entry {
    pub provider: String,
    pub error: String,
    pub plan: String,
    pub status: String,
    /// For example `none` when operational, `minor`, or `unknown`.
    pub status_level: String,
    pub updated_at: String,
    pub credits: Option<f64>,
    pub extra_usage: Option<ExtraUsage>,
    pub reset_credits: Option<ResetCredits>,
    pub windows: Vec<Window>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ExtraUsage {
    pub period: String,
    pub currency: String,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub balance: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ResetCredits {
    pub available: f64,
    pub next_expires_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Window {
    /// Opaque, stable for the lifetime of the desktop app process.
    pub key: String,
    pub label: String,
    pub minutes: Option<f64>,
    pub remaining: f64,
    /// Percentage in the user's used/remaining preference.
    pub display_value: f64,
    /// `left` or `used`.
    pub display_suffix: String,
    pub warning: bool,
    pub reset_text: String,
    pub pace: String,
    pub pace_expected: Option<f64>,
    pub pace_delta: Option<f64>,
    /// A per-model cap, such as "Fable only".
    pub scoped: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Spending {
    pub provider: String,
    pub today: Option<f64>,
    /// The last 30 days.
    pub month: Option<f64>,
    pub tokens: Option<f64>,
    pub provenance: String,
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: [(&str, &str); 10] = [
        (
            "bar-detail",
            include_str!("../tests/fixtures/snapshot-bar-detail.json"),
        ),
        (
            "bar-detail-spending",
            include_str!("../tests/fixtures/snapshot-bar-detail-spending.json"),
        ),
        (
            "one-provider",
            include_str!("../tests/fixtures/snapshot-one-provider.json"),
        ),
        (
            "one-provider-spending",
            include_str!("../tests/fixtures/snapshot-one-provider-spending.json"),
        ),
        (
            "provider-error",
            include_str!("../tests/fixtures/snapshot-provider-error.json"),
        ),
        (
            "provider-error-spending",
            include_str!("../tests/fixtures/snapshot-provider-error-spending.json"),
        ),
        (
            "quota-used",
            include_str!("../tests/fixtures/snapshot-quota-used.json"),
        ),
        (
            "quota-used-spending",
            include_str!("../tests/fixtures/snapshot-quota-used-spending.json"),
        ),
        (
            "two-providers",
            include_str!("../tests/fixtures/snapshot-two-providers.json"),
        ),
        (
            "two-providers-spending",
            include_str!("../tests/fixtures/snapshot-two-providers-spending.json"),
        ),
    ];

    fn fixture(name: &str) -> Snapshot {
        let (_, json) = FIXTURES.iter().find(|(n, _)| *n == name).unwrap();
        parse(json.as_bytes()).unwrap()
    }

    #[test]
    fn every_fixture_parses() {
        for (name, json) in FIXTURES {
            let snapshot = parse(json.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!snapshot.entries.is_empty(), "{name}: no entries");
            assert!(!snapshot.bar_entries.is_empty(), "{name}: no bar entries");
        }
    }

    #[test]
    fn fixtures_keep_their_distinguishing_fields() {
        let error = fixture("provider-error");
        assert!(
            error
                .entries
                .iter()
                .any(|e| e.provider == "gemini" && !e.error.is_empty())
        );
        assert_eq!(fixture("quota-used").quota_display, "used");
        assert!(fixture("bar-detail").bar_entries[0].text.contains(" · "));

        let two = fixture("two-providers-spending");
        let claude = &two.entries[0];
        assert_eq!(claude.windows.len(), 3);
        assert!(claude.windows[2].scoped);
        assert_eq!(claude.windows[0].pace_expected, Some(98.0));
        assert_eq!(claude.windows[2].pace_expected, None);
        let codex = &two.entries[1];
        assert_eq!(codex.credits, Some(0.0));
        assert_eq!(codex.extra_usage.as_ref().unwrap().balance, None);
        assert_eq!(codex.reset_credits.as_ref().unwrap().available, 3.0);
        let spending = two
            .spending
            .iter()
            .find(|s| s.provider == "claude")
            .unwrap();
        assert_eq!(spending.month, Some(42.5));
    }

    #[test]
    fn the_screenshot_snapshot_parses() {
        let snapshot = parse(include_bytes!("../resources/screenshots/snapshot.json")).unwrap();
        assert_eq!(snapshot.entries.len(), 2);
        assert_eq!(snapshot.spending.len(), 2);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = br#"{"schemaVersion":1,"summary":"CL 90%","futureField":{"a":[1]},
            "entries":[{"provider":"claude","newThing":true,
                        "windows":[{"label":"5 hour","displayValue":90,"resetsAt":"x","more":1}]}]}"#;
        let snapshot = parse(json).unwrap();
        assert_eq!(snapshot.summary, "CL 90%");
        assert_eq!(snapshot.entries[0].windows[0].display_value, 90.0);
    }

    #[test]
    fn missing_fields_default() {
        let snapshot = parse(br#"{"schemaVersion":1}"#).unwrap();
        assert_eq!(snapshot, Snapshot::default());
    }

    #[test]
    fn other_schema_versions_are_incompatible() {
        assert_eq!(
            parse(br#"{"schemaVersion":2}"#),
            Err(Incompatible::SchemaVersion(Some(2)))
        );
        assert_eq!(
            parse(br#"{"schemaVersion":0}"#),
            Err(Incompatible::SchemaVersion(Some(0)))
        );
        assert_eq!(
            parse(br#"{"summary":""}"#),
            Err(Incompatible::SchemaVersion(None))
        );
        assert_eq!(
            parse(br#"{"schemaVersion":"1"}"#),
            Err(Incompatible::SchemaVersion(None))
        );
        assert_eq!(parse(br"[1]"), Err(Incompatible::SchemaVersion(None)));
    }

    #[test]
    fn invalid_json_is_incompatible() {
        assert!(matches!(
            parse(b"{\"schemaVersion\":1"),
            Err(Incompatible::InvalidJson(_))
        ));
        assert!(matches!(
            parse(b"not json"),
            Err(Incompatible::InvalidJson(_))
        ));
        // Right version, wrong shape.
        assert!(matches!(
            parse(br#"{"schemaVersion":1,"entries":"nope"}"#),
            Err(Incompatible::InvalidJson(_))
        ));
    }

    fn tags_and_more(bar: Bar<'_>) -> (Vec<&str>, usize) {
        let Bar::Entries { entries, more } = bar else {
            panic!("{bar:?}")
        };
        (entries.iter().map(|e| e.tag.as_str()).collect(), more)
    }

    #[test]
    fn bar_shows_bar_entries_and_counts_the_rest() {
        let snapshot = fixture("provider-error");
        assert_eq!(tags_and_more(snapshot.bar(&[])), (vec!["CL", "CX"], 1));

        let snapshot = fixture("two-providers");
        assert!(matches!(snapshot.bar(&[]), Bar::Entries { more: 0, .. }));
    }

    #[test]
    fn hidden_providers_leave_the_bar_without_counting_as_more() {
        let snapshot = fixture("provider-error");
        let hidden = |ids: &[&str]| ids.iter().map(|&id| id.to_owned()).collect::<Vec<_>>();
        assert_eq!(
            tags_and_more(snapshot.bar(&hidden(&["claude"]))),
            (vec!["CX"], 1)
        );
        // Gemini isn't in the bar entries, so hiding it only drops the `+1`.
        assert_eq!(
            tags_and_more(snapshot.bar(&hidden(&["gemini"]))),
            (vec!["CL", "CX"], 0)
        );
        assert_eq!(snapshot.bar(&hidden(&["claude", "codex"])), Bar::Empty);
    }

    #[test]
    fn meters_are_the_first_three_windows_scoped_caps_last() {
        let snapshot = fixture("two-providers");
        let values = |provider| -> Vec<f64> {
            snapshot
                .meters(provider)
                .iter()
                .map(|meter| meter.value)
                .collect()
        };
        // Claude's third window is the "Fable only" scoped cap.
        assert_eq!(values("claude"), [90.0, 84.0, 93.0]);
        assert_eq!(values("codex"), [100.0, 100.0]);
        assert_eq!(values("gemini"), Vec::<f64>::new());

        let scoped_first = parse(
            br#"{"schemaVersion":1,"entries":[{"provider":"x","windows":[
                {"displayValue":5,"scoped":true},{"displayValue":10,"warning":true},
                {"displayValue":20},{"displayValue":30}]}]}"#,
        )
        .unwrap();
        let meters = scoped_first.meters("x");
        assert_eq!(
            meters.iter().map(|meter| meter.value).collect::<Vec<_>>(),
            [10.0, 20.0, 30.0]
        );
        assert!(meters[0].warning && !meters[1].warning);
    }

    #[test]
    fn bar_falls_back_to_summary_without_bar_entries() {
        let mut value: serde_json::Value = serde_json::from_str(
            FIXTURES
                .iter()
                .find(|(n, _)| *n == "provider-error")
                .unwrap()
                .1,
        )
        .unwrap();
        value.as_object_mut().unwrap().remove("barEntries");
        let snapshot = parse(value.to_string().as_bytes()).unwrap();
        assert_eq!(snapshot.bar(&[]), Bar::Summary("CL 90%  ·  CX 100%  +1"));
    }

    #[test]
    fn bar_is_empty_while_the_desktop_app_starts() {
        let snapshot =
            parse(br#"{"schemaVersion":1,"summary":"","barEntries":[],"entries":[]}"#).unwrap();
        assert_eq!(snapshot.bar(&[]), Bar::Empty);
    }
}
