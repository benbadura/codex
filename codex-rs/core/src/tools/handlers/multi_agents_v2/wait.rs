use super::*;
use crate::session::InputQueueActivity;
use crate::tools::handlers::multi_agents_spec::WaitAgentTimeoutOptions;
use crate::tools::handlers::multi_agents_spec::create_wait_agent_tool_v2;
use codex_tools::ToolSpec;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::Instant;
use tokio::time::timeout_at;

#[derive(Default)]
pub(crate) struct Handler {
    options: WaitAgentTimeoutOptions,
}

impl Handler {
    pub(crate) fn new(options: WaitAgentTimeoutOptions) -> Self {
        Self { options }
    }
}

impl ToolExecutor<ToolInvocation> for Handler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("wait_agent")
    }

    fn spec(&self) -> ToolSpec {
        create_wait_agent_tool_v2(self.options)
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(self.handle_call(invocation))
    }
}

impl Handler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            call_id,
            ..
        } = invocation;
        let arguments = function_arguments(payload)?;
        let args: WaitArgs = parse_arguments(&arguments)?;
        if args.mode == WaitMode::UntilEvent && args.timeout_ms.is_some() {
            return Err(FunctionCallError::RespondToModel(
                "until_event does not accept timeout_ms".to_string(),
            ));
        }
        let min_timeout_ms = turn.config.multi_agent_v2.min_wait_timeout_ms;
        let max_timeout_ms = turn.config.multi_agent_v2.max_wait_timeout_ms;
        let default_timeout_ms = turn.config.multi_agent_v2.default_wait_timeout_ms;
        let requested_timeout_ms = args.timeout_ms;
        let timeout_ms = match requested_timeout_ms {
            Some(ms) if ms > max_timeout_ms => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "timeout_ms must be at most {max_timeout_ms}"
                )));
            }
            Some(ms) => ms.max(min_timeout_ms),
            None => default_timeout_ms,
        };

        let turn_state = session
            .input_queue
            .turn_state_for_sub_id(&session.active_turn, &turn.sub_id)
            .await;
        let (mut activity_rx, pending_activity) = session
            .input_queue
            .subscribe_activity(turn_state.as_deref())
            .await;

        session
            .emit_turn_item_started(
                &turn,
                &TurnItem::CollabAgentToolCall(CollabAgentToolCallItem {
                    id: call_id.clone(),
                    tool: CollabAgentTool::Wait,
                    status: CollabAgentToolCallStatus::InProgress,
                    sender_thread_id: session.thread_id,
                    receiver_thread_ids: Vec::new(),
                    receiver_agents: Vec::new(),
                    prompt: None,
                    model: None,
                    reasoning_effort: None,
                    agents_states: Default::default(),
                }),
            )
            .await;

        let deadline = match args.mode {
            WaitMode::Timeout => Some(Instant::now() + Duration::from_millis(timeout_ms as u64)),
            WaitMode::UntilEvent => None,
        };
        let outcome = async {
            if args.mode == WaitMode::UntilEvent && pending_activity.is_none() {
                session
                    .services
                    .local_agent_runtime
                    .register_session_root(session.thread_id, turn.parent_thread_id);
                let agents = session
                    .services
                    .agent_control
                    .list(&turn.session_source, /*path_prefix*/ None)
                    .await
                    .map_err(collab_spawn_error)?;
                let has_active_agents = agents.iter().any(|agent| {
                    agent.thread_id != session.thread_id
                        && matches!(
                            agent.status,
                            AgentStatus::PendingInit | AgentStatus::Running
                        )
                });
                // Subscribe before inspecting liveness, then recheck the queue so a completion
                // arriving during the inspection is delivered instead of reporting no work.
                if has_active_agents {
                    wait_for_activity(&mut activity_rx, pending_activity, deadline).await
                } else {
                    let (_, pending_activity) = session
                        .input_queue
                        .subscribe_activity(turn_state.as_deref())
                        .await;
                    Ok(match pending_activity {
                        Some(InputQueueActivity::Mailbox) => WaitOutcome::MailboxActivity,
                        Some(InputQueueActivity::Steer) => WaitOutcome::Steered,
                        None => WaitOutcome::NoActiveAgents,
                    })
                }
            } else {
                wait_for_activity(&mut activity_rx, pending_activity, deadline).await
            }
        }
        .await;

        session
            .emit_turn_item_completed(
                &turn,
                TurnItem::CollabAgentToolCall(CollabAgentToolCallItem {
                    id: call_id,
                    tool: CollabAgentTool::Wait,
                    status: if outcome.is_ok() {
                        CollabAgentToolCallStatus::Completed
                    } else {
                        CollabAgentToolCallStatus::Failed
                    },
                    sender_thread_id: session.thread_id,
                    receiver_thread_ids: Vec::new(),
                    receiver_agents: Vec::new(),
                    prompt: None,
                    model: None,
                    reasoning_effort: None,
                    agents_states: HashMap::new(),
                }),
            )
            .await;

        Ok(boxed_tool_output(WaitAgentResult::from_outcome(
            outcome?,
            requested_timeout_ms,
            timeout_ms,
        )))
    }
}

impl CoreToolRuntime for Handler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitArgs {
    #[serde(default)]
    mode: WaitMode,
    timeout_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WaitMode {
    #[default]
    Timeout,
    UntilEvent,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct WaitAgentResult {
    pub(crate) message: String,
    pub(crate) timed_out: bool,
}

impl WaitAgentResult {
    fn from_outcome(
        outcome: WaitOutcome,
        requested_timeout_ms: Option<i64>,
        timeout_ms: i64,
    ) -> Self {
        let message = match outcome {
            WaitOutcome::MailboxActivity => "Wait completed.",
            WaitOutcome::Steered => "Wait interrupted by new input.",
            WaitOutcome::TimedOut => "Wait timed out.",
            WaitOutcome::NoActiveAgents => "no_active_agents",
        };
        let message = match requested_timeout_ms {
            Some(requested_timeout_ms) if requested_timeout_ms < timeout_ms => format!(
                "{message}\n\nRequested timeout of {requested_timeout_ms}ms was clamped to the minimum of {timeout_ms}ms."
            ),
            Some(_) | None => message.to_string(),
        };
        Self {
            message,
            timed_out: outcome == WaitOutcome::TimedOut,
        }
    }
}

impl ToolOutput for WaitAgentResult {
    fn log_output(&self) -> String {
        tool_output_json_text(self, "wait_agent")
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        tool_output_response_item(call_id, payload, self, /*success*/ None, "wait_agent")
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        tool_output_code_mode_result(self, "wait_agent")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaitOutcome {
    MailboxActivity,
    Steered,
    TimedOut,
    NoActiveAgents,
}

async fn wait_for_activity(
    activity_rx: &mut tokio::sync::watch::Receiver<InputQueueActivity>,
    pending_activity: Option<InputQueueActivity>,
    deadline: Option<Instant>,
) -> Result<WaitOutcome, FunctionCallError> {
    if let Some(activity) = pending_activity {
        return Ok(match activity {
            InputQueueActivity::Mailbox => WaitOutcome::MailboxActivity,
            InputQueueActivity::Steer => WaitOutcome::Steered,
        });
    }
    let changed = match deadline {
        Some(deadline) => match timeout_at(deadline, activity_rx.changed()).await {
            Ok(changed) => changed,
            Err(_) => return Ok(WaitOutcome::TimedOut),
        },
        None => activity_rx.changed().await,
    };
    match changed {
        Ok(()) => Ok(match *activity_rx.borrow_and_update() {
            InputQueueActivity::Mailbox => WaitOutcome::MailboxActivity,
            InputQueueActivity::Steer => WaitOutcome::Steered,
        }),
        Err(_) => Err(FunctionCallError::RespondToModel(
            "Agent activity channel closed.".to_string(),
        )),
    }
}

#[cfg(test)]
#[path = "wait_tests.rs"]
mod tests;
