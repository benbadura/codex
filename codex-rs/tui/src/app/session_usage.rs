//! UI-only accounting from cumulative usage notifications. No model calls or polling.
//! Retains completed agents; unknown historical model attribution is never guessed.

#[cfg(test)]
#[path = "session_usage_tests.rs"]
mod tests;
#[path = "session_usage_view.rs"]
mod view;

use super::App;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_protocol::openai_models::ReasoningEffort;
use std::collections::BTreeMap;
use std::time::Instant;

const MAX_THREADS: usize = 4096;
const MAX_MODELS: usize = 64;
const MAX_TURNS: usize = 2048;
const UNKNOWN: &str = "unknown / before observation";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tokens {
    input: i64,
    cached: i64,
    write: i64,
    output: i64,
    reasoning: i64,
}

impl Tokens {
    fn from_usage(usage: &TokenUsageBreakdown) -> Self {
        Self {
            input: usage.input_tokens.max(0),
            cached: usage.cached_input_tokens.max(0),
            write: usage.cache_write_input_tokens.max(0),
            output: usage.output_tokens.max(0),
            reasoning: usage.reasoning_output_tokens.max(0),
        }
    }

    fn add(&mut self, other: Self) {
        self.input = self.input.saturating_add(other.input);
        self.cached = self.cached.saturating_add(other.cached);
        self.write = self.write.saturating_add(other.write);
        self.output = self.output.saturating_add(other.output);
        self.reasoning = self.reasoning.saturating_add(other.reasoning);
    }

    fn subtract(self, other: Self) -> Self {
        Self {
            input: self.input.saturating_sub(other.input).max(0),
            cached: self.cached.saturating_sub(other.cached).max(0),
            write: self.write.saturating_sub(other.write).max(0),
            output: self.output.saturating_sub(other.output).max(0),
            reasoning: self.reasoning.saturating_sub(other.reasoning).max(0),
        }
    }
}

#[derive(Default)]
struct AgentUsage {
    parent: Option<String>,
    name: String,
    model: String,
    reasoning_effort: Option<ReasoningEffort>,
    status: String,
    previous: Option<Tokens>,
    models: BTreeMap<String, Tokens>,
    turns: BTreeMap<String, (Instant, Option<u64>)>,
    reset: bool,
    metadata_requested: bool,
}

impl AgentUsage {
    fn record(&mut self, model: String, tokens: Tokens) {
        if tokens == Tokens::default() {
            return;
        }
        let model = if self.models.len() >= MAX_MODELS && !self.models.contains_key(&model) {
            UNKNOWN.to_string()
        } else {
            model
        };
        self.models.entry(model).or_default().add(tokens);
    }

    fn update(&mut self, total: Tokens, last: Tokens, turn: &str) {
        if let Some(previous) = self.previous {
            // Repeated snapshots are not additional consumption. Reject stale totals.
            if total.input < previous.input
                || total.output < previous.output
                || total.cached < previous.cached
                || total.write < previous.write
                || total.reasoning < previous.reasoning
            {
                return;
            }
            let delta = total.subtract(previous);
            self.record(self.model.clone(), delta);
        } else if !self.reset {
            // Only a turn observed live can attribute its last response to a model.
            // Earlier totals on resume may contain several models.
            let live = self.turns.contains_key(turn);
            let recent = if live {
                total.subtract(total.subtract(last))
            } else {
                Tokens::default()
            };
            self.record(UNKNOWN.to_string(), total.subtract(recent));
            self.record(self.model.clone(), recent);
        }
        self.previous = Some(total);
        self.reset = false;
    }

    fn elapsed_ms(&self, now: Instant) -> u64 {
        self.turns
            .values()
            .map(|(start, done)| {
                done.unwrap_or_else(|| now.saturating_duration_since(*start).as_millis() as u64)
            })
            .fold(/*init*/ 0, u64::saturating_add)
    }
}

#[derive(Default)]
pub(super) struct SessionUsage {
    agents: BTreeMap<String, AgentUsage>,
    pub(super) incomplete: bool,
}

impl SessionUsage {
    fn metadata(&mut self, thread: &Thread) {
        if self.agents.len() >= MAX_THREADS && !self.agents.contains_key(&thread.id) {
            self.incomplete = true;
            return;
        }
        let agent = self.agents.entry(thread.id.clone()).or_default();
        agent.metadata_requested = true;
        agent.parent.clone_from(&thread.parent_thread_id);
        if let codex_app_server_protocol::SessionSource::SubAgent(
            codex_protocol::protocol::SubAgentSource::ThreadSpawn {
                parent_thread_id,
                agent_path,
                ..
            },
        ) = &thread.source
        {
            agent.parent = Some(parent_thread_id.to_string());
            if let Some(path) = agent_path {
                agent.name = path.to_string();
            }
        }
        if agent.name.is_empty() {
            agent.name = thread
                .agent_nickname
                .as_deref()
                .or(thread.name.as_deref())
                .unwrap_or(&thread.id)
                .chars()
                .take(/*n*/ 120)
                .collect();
        }
        if let Some(model) = &thread.model {
            agent.model.clone_from(model);
        } else if agent.model.is_empty() {
            agent.model = UNKNOWN.to_string();
        }
        agent.reasoning_effort.clone_from(&thread.reasoning_effort);
        agent.status = format!("{:?}", thread.status);
    }

    fn observe(&mut self, notification: &ServerNotification) {
        use super::app_server_event_targets::ServerNotificationThreadTarget;
        use super::app_server_event_targets::server_notification_thread_target;
        if let ServerNotification::ThreadStarted(event) = notification {
            self.metadata(&event.thread);
        }
        let ServerNotificationThreadTarget::Thread(id) =
            server_notification_thread_target(notification)
        else {
            return;
        };
        let id = id.to_string();
        if !self.agents.contains_key(&id) && self.agents.len() >= MAX_THREADS {
            self.incomplete = true;
            return;
        }
        let agent = self.agents.entry(id).or_insert_with(|| AgentUsage {
            model: UNKNOWN.to_string(),
            ..Default::default()
        });
        match notification {
            ServerNotification::ThreadTokenUsageUpdated(event) => agent.update(
                Tokens::from_usage(&event.token_usage.total),
                Tokens::from_usage(&event.token_usage.last),
                &event.turn_id,
            ),
            ServerNotification::ThreadSettingsUpdated(event) => {
                agent.model.clone_from(&event.thread_settings.model);
                agent
                    .reasoning_effort
                    .clone_from(&event.thread_settings.effort);
            }
            ServerNotification::ModelRerouted(event) => agent.model.clone_from(&event.to_model),
            ServerNotification::TurnStarted(event) => {
                if agent.turns.len() < MAX_TURNS {
                    agent
                        .turns
                        .entry(event.turn.id.clone())
                        .or_insert((Instant::now(), None));
                } else {
                    self.incomplete = true;
                }
                agent.status = "Running".into();
            }
            ServerNotification::TurnCompleted(event) => {
                if let Some((start, done)) = agent.turns.get_mut(&event.turn.id) {
                    *done = Some(
                        event
                            .turn
                            .duration_ms
                            .and_then(|ms| u64::try_from(ms).ok())
                            .unwrap_or_else(|| start.elapsed().as_millis() as u64),
                    );
                }
                agent.status = format!("{:?}", event.turn.status);
            }
            ServerNotification::ThreadStatusChanged(event) => {
                agent.status = format!("{:?}", event.status);
                if !matches!(
                    event.status,
                    codex_app_server_protocol::ThreadStatus::Active { .. }
                ) {
                    for (start, done) in agent.turns.values_mut() {
                        done.get_or_insert_with(|| start.elapsed().as_millis() as u64);
                    }
                }
            }
            ServerNotification::ThreadClosed(_)
            | ServerNotification::ThreadArchived(_)
            | ServerNotification::ThreadDeleted(_) => {
                agent.status = "Closed".into();
                for (start, done) in agent.turns.values_mut() {
                    done.get_or_insert_with(|| start.elapsed().as_millis() as u64);
                }
            }
            ServerNotification::ThreadReverted(_) => {
                agent.previous = None;
                agent.reset = true;
                self.incomplete = true;
            }
            _ => {}
        }
    }

    pub(in crate::app) fn root(&self, id: &str) -> String {
        let mut root = id;
        for _ in 0..self.agents.len() {
            let Some(parent) = self
                .agents
                .get(root)
                .and_then(|agent| agent.parent.as_deref())
            else {
                break;
            };
            root = parent;
        }
        root.to_string()
    }
}

impl App {
    pub(super) fn track_session_usage(
        &mut self,
        notification: &ServerNotification,
        app_server: &crate::app_server_session::AppServerSession,
    ) {
        let mut usage = self
            .session_usage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The primary thread may have been attached before the notification subscription.
        if let Some(id) = self.chat_widget.thread_id() {
            let selected_is_primary = self.primary_thread_id == Some(id);
            let agent = usage.agents.entry(id.to_string()).or_default();
            if agent.model.is_empty() {
                agent.model = self.chat_widget.current_model().to_string();
            }
            if agent.reasoning_effort.is_none() {
                agent.reasoning_effort = self.chat_widget.current_reasoning_effort();
            }
            // The primary thread is already represented by ChatWidget. Avoid a redundant
            // thread/read whose timeout would incorrectly mark otherwise complete live usage
            // as partial.
            if selected_is_primary {
                agent.metadata_requested = true;
            }
        }
        usage.observe(notification);
        // Auto-attached subagents can emit turns without thread/started. Resolve their
        // ownership and model once, independently of whether the dashboard is open.
        let super::app_server_event_targets::ServerNotificationThreadTarget::Thread(id) =
            super::app_server_event_targets::server_notification_thread_target(notification)
        else {
            return;
        };
        let Some(agent) = usage.agents.get_mut(&id.to_string()) else {
            return;
        };
        if agent.metadata_requested {
            return;
        }
        agent.metadata_requested = true;
        let request = app_server.request_handle();
        let ledger = std::sync::Arc::clone(&self.session_usage);
        tokio::spawn(async move {
            let response = tokio::time::timeout(
                std::time::Duration::from_secs(/*secs*/ 5),
                request.request_typed::<codex_app_server_protocol::ThreadReadResponse>(
                    codex_app_server_protocol::ClientRequest::ThreadRead {
                        request_id: codex_app_server_protocol::RequestId::String(format!(
                            "session-usage-{id}"
                        )),
                        params: codex_app_server_protocol::ThreadReadParams {
                            thread_id: id.to_string(),
                            include_turns: false,
                        },
                    },
                ),
            )
            .await;
            let mut usage = ledger
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match response {
                Ok(Ok(mut response)) => {
                    // A live settings notification received during the read is newer.
                    if let Some(agent) = usage.agents.get(&id.to_string())
                        && !agent.model.is_empty()
                        && agent.model != UNKNOWN
                    {
                        response.thread.model = None;
                    }
                    let status = usage
                        .agents
                        .get(&id.to_string())
                        .map(|agent| agent.status.clone());
                    usage.metadata(&response.thread);
                    if let Some(status) = status
                        && let Some(agent) = usage.agents.get_mut(&id.to_string())
                    {
                        agent.status = status;
                    }
                }
                Ok(Err(_)) | Err(_) => usage.incomplete = true,
            }
        });
    }
}
