//! Event waits must not sample the parent model for elapsed time or UI-only progress.
use super::subagent_notifications::body_contains;
use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_features::Feature;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SubAgentActivityKind;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_response_once_match;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::sse;
use core_test_support::responses::sse_response;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use test_case::test_case;

const ROOT_PROMPT: &str = "parent waiting for event worker";
const CHILD_PROMPT: &str = "event worker reports progress";
const PROGRESS_TEXT: &str = "private-progress-sentinel";

#[derive(Clone, Copy)]
enum Outcome {
    Completed,
    Failed,
    Message,
    Cancelled,
    Steered,
}

#[test_case(Outcome::Completed; "completion")]
#[test_case(Outcome::Failed; "failure")]
#[test_case(Outcome::Message; "actionable_message")]
#[test_case(Outcome::Cancelled; "cancellation")]
#[test_case(Outcome::Steered; "steering")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn progress_and_elapsed_time_do_not_sample_parent(outcome: Outcome) -> Result<()> {
    let server = start_mock_server().await;
    mount_sse_once_match(
        &server,
        |req: &wiremock::Request| body_contains(req, ROOT_PROMPT),
        sse(vec![
            ev_response_created("parent-spawn"),
            ev_function_call_with_namespace(
                "spawn-event-worker",
                "collaboration",
                "spawn_agent",
                &json!({
                    "task_name": "worker", "message": CHILD_PROMPT, "fork_turns": "none",
                })
                .to_string(),
            ),
            ev_completed("parent-spawn"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        |req: &wiremock::Request| {
            body_contains(req, ROOT_PROMPT) && body_contains(req, "spawn-event-worker")
        },
        sse(vec![
            ev_response_created("parent-wait"),
            ev_function_call_with_namespace(
                "event-wait",
                "collaboration",
                "wait_agent",
                r#"{"mode":"until_event"}"#,
            ),
            ev_completed("parent-wait"),
        ]),
    )
    .await;
    let parent_done = mount_sse_once_match(
        &server,
        |req: &wiremock::Request| {
            body_contains(req, ROOT_PROMPT) && body_contains(req, "event-wait")
        },
        sse(vec![
            ev_response_created("parent-done"),
            ev_assistant_message("done", "done"),
            ev_completed("parent-done"),
        ]),
    )
    .await;
    for (index, previous) in [(1, None), (2, Some("progress-1"))] {
        mount_response_once_match(
            &server,
            move |req: &wiremock::Request| {
                body_contains(req, CHILD_PROMPT)
                    && !body_contains(req, ROOT_PROMPT)
                    && previous.is_none_or(|previous| body_contains(req, previous))
            },
            sse_response(sse(vec![
                ev_response_created(&format!("child-{index}")),
                ev_function_call_with_namespace(
                    &format!("progress-{index}"),
                    "collaboration",
                    "send_message",
                    &json!({
                        "target": "/root", "message": PROGRESS_TEXT, "kind": "progress",
                    })
                    .to_string(),
                ),
                ev_completed(&format!("child-{index}")),
            ]))
            .set_delay(Duration::from_millis(100)),
        )
        .await;
    }
    let child_events = match outcome {
        Outcome::Failed => vec![ev_response_created("child-error")],
        Outcome::Message => vec![
            ev_response_created("child-blocked"),
            ev_function_call_with_namespace(
                "blocker",
                "collaboration",
                "send_message",
                r#"{"target":"/root","message":"Need a decision before continuing"}"#,
            ),
            ev_completed("child-blocked"),
        ],
        Outcome::Completed | Outcome::Cancelled | Outcome::Steered => vec![
            ev_response_created("child-done"),
            ev_assistant_message("result", "worker result"),
            ev_completed("child-done"),
        ],
    };
    mount_response_once_match(
        &server,
        |req: &wiremock::Request| {
            body_contains(req, CHILD_PROMPT)
                && !body_contains(req, ROOT_PROMPT)
                && body_contains(req, "progress-2")
        },
        sse_response(sse(child_events)).set_delay(Duration::from_millis(500)),
    )
    .await;
    if matches!(outcome, Outcome::Message) {
        mount_response_once_match(
            &server,
            |req: &wiremock::Request| {
                body_contains(req, CHILD_PROMPT)
                    && !body_contains(req, ROOT_PROMPT)
                    && body_contains(req, "blocker")
            },
            sse_response(sse(vec![
                ev_response_created("child-done"),
                ev_assistant_message("result", "worker result"),
                ev_completed("child-done"),
            ]))
            .set_delay(Duration::from_millis(500)),
        )
        .await;
    }
    let test = test_codex()
        .with_model("gpt-5.6-sol")
        .with_config(|config| {
            config
                .features
                .enable(Feature::Collab)
                .expect("enable collaboration");
            config
                .features
                .enable(Feature::MultiAgentV2)
                .expect("enable multi-agent v2");
            config.multi_agent_v2.min_wait_timeout_ms = 1;
            config.multi_agent_v2.default_wait_timeout_ms = 10;
            config.model_provider.supports_websockets = false;
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: ROOT_PROMPT.to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    for _ in 0..2 {
        wait_for_event(&test.codex, |event| matches!(event,
            EventMsg::ItemCompleted(event) if matches!(&event.item,
                TurnItem::SubAgentActivity(activity) if activity.kind == SubAgentActivityKind::Progress
            )
        )).await;
    }
    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(
        requests
            .iter()
            .filter(|req| body_contains(req, ROOT_PROMPT))
            .count(),
        2
    );
    match outcome {
        Outcome::Cancelled => {
            test.codex.submit(Op::Interrupt).await?;
            wait_for_event(&test.codex, |event| {
                matches!(event, EventMsg::TurnAborted(_))
            })
            .await;
            let requests = server.received_requests().await.expect("recorded requests");
            assert_eq!(
                requests
                    .iter()
                    .filter(|req| body_contains(req, ROOT_PROMPT))
                    .count(),
                2
            );
            return Ok(());
        }
        Outcome::Steered => {
            test.codex
                .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "Stop waiting and summarize now".to_string(),
                    text_elements: Vec::new(),
                }]))
                .await?;
        }
        Outcome::Completed | Outcome::Failed | Outcome::Message => {}
    }
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let requests = server.received_requests().await.expect("recorded requests");
    let parent_requests: Vec<_> = requests
        .iter()
        .filter(|req| body_contains(req, ROOT_PROMPT))
        .collect();
    assert_eq!(parent_requests.len(), 3);
    assert!(
        parent_requests
            .iter()
            .all(|req| !body_contains(req, PROGRESS_TEXT))
    );
    let final_request = parent_done
        .requests()
        .into_iter()
        .find(|req| req.function_call_output_text("event-wait").is_some())
        .expect("wait output");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &final_request
                .function_call_output_text("event-wait")
                .expect("wait tool output")
        )?,
        json!({"message": if matches!(outcome, Outcome::Steered) { "Wait interrupted by new input." } else { "Wait completed." }, "timed_out": false})
    );
    let expected = match outcome {
        Outcome::Failed => "Agent errored:",
        Outcome::Message => "Need a decision before continuing",
        Outcome::Steered => "Stop waiting and summarize now",
        Outcome::Completed | Outcome::Cancelled => "worker result",
    };
    assert!(final_request.body_json().to_string().contains(expected));
    Ok(())
}

#[test_case(json!({"mode": "until_event"}), "no_active_agents"; "no_workers")]
#[test_case(json!({"mode": "until_event", "timeout_ms": 1000}), "until_event does not accept timeout_ms"; "invalid_timeout")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn event_wait_without_work_or_with_invalid_timeout_finishes(
    arguments: serde_json::Value,
    expected: &str,
) -> Result<()> {
    let server = start_mock_server().await;
    core_test_support::responses::mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("wait"),
            ev_function_call_with_namespace(
                "event-wait",
                "collaboration",
                "wait_agent",
                &arguments.to_string(),
            ),
            ev_completed("wait"),
        ]),
    )
    .await;
    let done = mount_sse_once_match(
        &server,
        |req: &wiremock::Request| body_contains(req, "event-wait"),
        sse(vec![
            ev_response_created("done"),
            ev_assistant_message("done", "done"),
            ev_completed("done"),
        ]),
    )
    .await;
    let test = test_codex()
        .with_model("gpt-5.6-sol")
        .with_config(|config| {
            config
                .features
                .enable(Feature::Collab)
                .expect("enable collaboration");
            config
                .features
                .enable(Feature::MultiAgentV2)
                .expect("enable multi-agent v2");
            config.model_provider.supports_websockets = false;
        })
        .build_with_auto_env(&server)
        .await?;
    test.submit_turn("wait with no workers").await?;
    assert!(
        done.function_call_output_text("event-wait")
            .expect("wait tool output")
            .contains(expected)
    );
    Ok(())
}
