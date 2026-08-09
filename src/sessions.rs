// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::app::Message;
use crate::config::SessionLabel;
use crate::fl;
use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget;

#[derive(Debug, Default, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentSession {
    pub id: String,
    pub host: String,
    pub project_name: String,
    pub session_name: String,
    pub state: String,
    pub provider: String,
    pub cwd: String,
    pub source: String,
    pub started_at: String,
    pub last_activity_at: String,
}

impl AgentSession {
    pub fn active(&self) -> bool {
        self.state.eq_ignore_ascii_case("active")
    }

    pub fn label(&self, mode: SessionLabel) -> String {
        let project = self.project();
        let descriptive = if self.session_name.trim().is_empty() {
            project
        } else {
            self.session_name.trim()
        };
        match mode {
            SessionLabel::Project => project.to_owned(),
            SessionLabel::Descriptive => descriptive.to_owned(),
            SessionLabel::DescriptiveProject if descriptive != project => {
                format!("{descriptive} · {project}")
            }
            SessionLabel::DescriptiveProject => project.to_owned(),
        }
    }

    fn project(&self) -> &str {
        if !self.project_name.trim().is_empty() {
            return self.project_name.trim();
        }
        Path::new(&self.cwd)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Unknown project")
    }
}

#[derive(Debug)]
pub struct Group<'a> {
    pub host: &'a str,
    pub local: bool,
    pub sessions: Vec<&'a AgentSession>,
}

pub fn grouped<'a>(sessions: &'a [AgentSession], local_host: &'a str) -> Vec<Group<'a>> {
    let mut local = Vec::new();
    let mut remote = BTreeMap::<&str, Vec<&AgentSession>>::new();
    for session in sessions {
        if is_local_host(&session.host, local_host) {
            local.push(session);
        } else {
            remote.entry(&session.host).or_default().push(session);
        }
    }

    let mut groups = Vec::with_capacity(remote.len() + usize::from(!local.is_empty()));
    if !local.is_empty() {
        groups.push(Group {
            host: local_host,
            local: true,
            sessions: local,
        });
    }
    groups.extend(remote.into_iter().map(|(host, sessions)| Group {
        host,
        local: false,
        sessions,
    }));
    groups
}

fn is_local_host(host: &str, local_host: &str) -> bool {
    let host = host.trim_end_matches('.');
    let local_host = local_host.trim_end_matches('.');
    host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case(local_host)
        || host
            .split('.')
            .next()
            .zip(local_host.split('.').next())
            .is_some_and(|(host, local)| host.eq_ignore_ascii_case(local))
}

pub fn should_poll(popup_open: bool) -> bool {
    popup_open
}

#[derive(Debug, Eq, PartialEq)]
pub struct TerminalLaunch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
}

pub fn terminal_launch(
    terminal: &str,
    session: &AgentSession,
    local_host: &str,
) -> Result<TerminalLaunch, String> {
    if !is_local_host(&session.host, local_host) {
        return Err(format!(
            "cannot open a session on remote host {}",
            session.host
        ));
    }
    if terminal.trim().is_empty() {
        return Err("terminal app is not configured".into());
    }
    if session.cwd.is_empty() {
        return Err("session has no working directory".into());
    }
    Ok(TerminalLaunch {
        program: terminal.to_owned(),
        args: Vec::new(),
        cwd: session.cwd.clone(),
    })
}

pub fn launch_terminal(
    terminal: &str,
    session: &AgentSession,
    local_host: &str,
) -> Result<(), String> {
    let launch = terminal_launch(terminal, session, local_host)?;
    Command::new(&launch.program)
        .args(&launch.args)
        .current_dir(&launch.cwd)
        .spawn()
        .map(drop)
        .map_err(|error| format!("failed to launch {}: {error}", launch.program))
}

#[derive(Debug, Default)]
pub struct State {
    pub sessions: Vec<AgentSession>,
    pub error: Option<String>,
    pub refreshing: bool,
}

impl State {
    pub fn request_refresh(&mut self) -> bool {
        if self.refreshing {
            return false;
        }
        self.refreshing = true;
        true
    }

    pub fn finish(&mut self, result: Result<Vec<AgentSession>, String>) {
        self.refreshing = false;
        match result {
            Ok(sessions) => {
                self.sessions = sessions;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

pub fn local_host() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|host| !host.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|host| host.trim().to_owned())
                .filter(|host| !host.is_empty())
        })
        .unwrap_or_else(|| "localhost".into())
}

pub fn view<'a>(state: &'a State, local_host: &'a str, mode: SessionLabel) -> Element<'a, Message> {
    let mut content = widget::column::with_capacity(state.sessions.len() + 3)
        .push(widget::text::heading(fl!("sessions-title")))
        .spacing(8);

    if state.refreshing && state.sessions.is_empty() {
        content = content.push(widget::text::body(fl!("sessions-loading")));
    } else if state.sessions.is_empty() {
        content = content.push(widget::text::body(fl!("sessions-empty")));
    }

    for group in grouped(&state.sessions, local_host) {
        content = content.push(widget::text::caption_heading(if group.local {
            fl!("sessions-local")
        } else {
            group.host.to_owned()
        }));
        for session in group.sessions {
            let state_label = if session.active() {
                fl!("sessions-active")
            } else {
                fl!("sessions-idle")
            };
            let row = format!(
                "{} {state_label} · {}",
                if session.active() { "●" } else { "○" },
                session.label(mode)
            );
            if group.local {
                content = content.push(
                    widget::button::text(row)
                        .on_press(Message::OpenSession(session.id.clone()))
                        .width(Length::Fill),
                );
            } else {
                content = content.push(widget::text::body(row));
            }
        }
    }

    if let Some(error) = &state.error {
        content = content
            .push(widget::text::caption(fl!("sessions-stale")))
            .push(widget::text::caption(error));
    }

    widget::container(content)
        .padding([4, 16, 12, 16])
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(host: &str, project: &str, name: &str, state: &str) -> AgentSession {
        AgentSession {
            id: format!("{host}-{project}"),
            host: host.into(),
            project_name: project.into(),
            session_name: name.into(),
            state: state.into(),
            provider: "codex".into(),
            cwd: format!("/work/{project}"),
            ..Default::default()
        }
    }

    #[test]
    fn parses_live_sessions_payload() {
        let sessions: Vec<AgentSession> = serde_json::from_str(
            r#"[{"lastActivityAt":"2026-08-09T12:36:15Z","state":"active","cwd":"/work/codexbar","provider":"codex","id":"session-1","source":"unknown","projectName":"codexbar","sessionName":"Implement sessions","host":"laptop"}]"#,
        )
        .unwrap();

        assert_eq!(sessions[0].project_name, "codexbar");
        assert_eq!(sessions[0].session_name, "Implement sessions");
        assert_eq!(sessions[0].last_activity_at, "2026-08-09T12:36:15Z");
        assert!(sessions[0].active());
    }

    #[test]
    fn groups_local_first_then_remote_hosts_and_formats_labels() {
        let sessions = vec![
            session("zeta", "remote-z", "Remote Z", "idle"),
            session("laptop", "local", "Local task", "active"),
            session("alpha", "remote-a", "Remote A", "active"),
            session("laptop", "same", "same", "idle"),
        ];
        let groups = grouped(&sessions, "laptop");

        assert_eq!(
            groups.iter().map(|group| group.host).collect::<Vec<_>>(),
            ["laptop", "alpha", "zeta"]
        );
        assert!(groups[0].local);
        assert!(!groups[1].local);
        assert_eq!(sessions[1].label(SessionLabel::Project), "local");
        assert_eq!(sessions[1].label(SessionLabel::Descriptive), "Local task");
        assert_eq!(
            sessions[1].label(SessionLabel::DescriptiveProject),
            "Local task · local"
        );
        assert_eq!(sessions[3].label(SessionLabel::DescriptiveProject), "same");
        assert!(is_local_host("laptop.example.org", "laptop"));
        assert!(is_local_host("localhost", "laptop"));
        assert!(!is_local_host("desktop", "laptop"));
    }

    #[test]
    fn polls_only_while_popup_is_open() {
        assert!(should_poll(true));
        assert!(!should_poll(false));
    }

    #[test]
    fn builds_shell_free_terminal_launch_and_refuses_remote_sessions() {
        let local = session("laptop", "local", "Local task", "active");
        let launch = terminal_launch("cosmic-term", &local, "laptop").unwrap();
        assert_eq!(launch.program, "cosmic-term");
        assert!(launch.args.is_empty());
        assert_eq!(launch.cwd, "/work/local");

        let remote = session("desktop", "remote", "Remote task", "active");
        assert!(terminal_launch("cosmic-term", &remote, "laptop").is_err());
    }

    #[test]
    fn failed_refresh_keeps_last_good_sessions() {
        let mut state = State::default();
        state.finish(Ok(vec![session("laptop", "local", "Task", "active")]));
        state.finish(Err("offline".into()));

        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.error.as_deref(), Some("offline"));
        assert!(!state.refreshing);
    }
}
