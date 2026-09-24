use super::*;
use codex_app_server_protocol::ThreadClosedNotification;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
use pretty_assertions::assert_eq;
use std::time::Duration;

fn usage(input: i64, cached: i64, output: i64) -> Tokens {
    Tokens {
        input,
        cached,
        output,
        ..Default::default()
    }
}

fn agent(model: &str) -> AgentUsage {
    AgentUsage {
        model: model.into(),
        turns: BTreeMap::from([("turn".into(), (Instant::now(), None))]),
        ..Default::default()
    }
}

#[test]
fn cumulative_updates_deduplicate_and_split_model_changes() {
    let mut agent = agent("astra");
    let first = usage(
        /*input*/ 1000, /*cached*/ 800, /*output*/ 100,
    );
    agent.update(first, first, "turn");
    agent.update(first, first, "turn");
    agent.model = "luna".into();
    agent.update(
        usage(
            /*input*/ 1300, /*cached*/ 1000, /*output*/ 150,
        ),
        usage(/*input*/ 300, /*cached*/ 200, /*output*/ 50),
        "turn",
    );
    // A delayed snapshot must not lower the baseline or count the next update twice.
    agent.update(first, first, "turn");
    agent.update(
        usage(
            /*input*/ 1300, /*cached*/ 1000, /*output*/ 150,
        ),
        usage(/*input*/ 300, /*cached*/ 200, /*output*/ 50),
        "turn",
    );
    agent.update(
        usage(
            /*input*/ 1300, /*cached*/ 900, /*output*/ 150,
        ),
        usage(/*input*/ 300, /*cached*/ 100, /*output*/ 50),
        "turn",
    );
    agent.update(
        usage(
            /*input*/ 1300, /*cached*/ 1000, /*output*/ 150,
        ),
        usage(/*input*/ 300, /*cached*/ 200, /*output*/ 50),
        "turn",
    );
    assert_eq!(
        agent.models,
        BTreeMap::from([
            ("astra".into(), first),
            (
                "luna".into(),
                usage(/*input*/ 300, /*cached*/ 200, /*output*/ 50)
            )
        ])
    );
}

#[test]
fn resume_keeps_unknown_history_separate_from_live_model() {
    let mut agent = agent("luna");
    agent.update(
        usage(
            /*input*/ 1300, /*cached*/ 1000, /*output*/ 150,
        ),
        usage(/*input*/ 300, /*cached*/ 200, /*output*/ 50),
        "turn",
    );
    assert_eq!(
        agent.models,
        BTreeMap::from([
            (
                UNKNOWN.into(),
                usage(
                    /*input*/ 1000, /*cached*/ 800, /*output*/ 100
                )
            ),
            (
                "luna".into(),
                usage(/*input*/ 300, /*cached*/ 200, /*output*/ 50)
            )
        ])
    );
    let mut cold = AgentUsage {
        model: "luna".into(),
        ..Default::default()
    };
    cold.update(
        usage(
            /*input*/ 1300, /*cached*/ 1000, /*output*/ 150,
        ),
        usage(/*input*/ 300, /*cached*/ 200, /*output*/ 50),
        "old-turn",
    );
    assert_eq!(
        cold.models,
        BTreeMap::from([(
            UNKNOWN.into(),
            usage(
                /*input*/ 1300, /*cached*/ 1000, /*output*/ 150
            )
        )])
    );
}

#[test]
fn close_retains_usage_and_stops_active_clock() {
    let id = codex_protocol::ThreadId::new().to_string();
    let mut worker = agent("luna");
    worker.update(
        usage(/*input*/ 200, /*cached*/ 100, /*output*/ 20),
        usage(/*input*/ 200, /*cached*/ 100, /*output*/ 20),
        "turn",
    );
    let mut ledger = SessionUsage {
        agents: BTreeMap::from([(id.clone(), worker)]),
        ..Default::default()
    };
    ledger.observe(&ServerNotification::ThreadClosed(
        ThreadClosedNotification {
            thread_id: id.clone(),
        },
    ));
    let worker = &ledger.agents[&id];
    assert_eq!(
        worker.models,
        BTreeMap::from([(
            "luna".into(),
            usage(/*input*/ 200, /*cached*/ 100, /*output*/ 20)
        )])
    );
    let now = Instant::now();
    assert_eq!(
        worker.elapsed_ms(now),
        worker.elapsed_ms(now + Duration::from_secs(/*secs*/ 600))
    );
}

fn dashboard() -> (SessionUsage, Instant) {
    let now = Instant::now();
    let mut parent = agent("astra");
    parent.name = "Coordinator".into();
    parent.status = "Running".into();
    parent.turns = BTreeMap::from([(
        "turn".into(),
        (now - Duration::from_secs(/*secs*/ 10), None),
    )]);
    parent.update(
        usage(
            /*input*/ 1000, /*cached*/ 800, /*output*/ 100,
        ),
        usage(
            /*input*/ 1000, /*cached*/ 800, /*output*/ 100,
        ),
        "turn",
    );
    let mut child = agent("luna");
    child.parent = Some("root".into());
    child.name = "/root/tests".into();
    child.status = "Completed".into();
    child.turns = BTreeMap::from([("turn".into(), (now, Some(2500)))]);
    child.update(
        usage(/*input*/ 400, /*cached*/ 200, /*output*/ 50),
        usage(/*input*/ 400, /*cached*/ 200, /*output*/ 50),
        "turn",
    );
    let mut nested = agent("luna");
    nested.parent = Some("child".into());
    nested.name = "/root/tests/check".into();
    nested.status = "Completed".into();
    nested.turns = BTreeMap::from([("turn".into(), (now, Some(1000)))]);
    nested.update(
        usage(/*input*/ 200, /*cached*/ 100, /*output*/ 25),
        usage(/*input*/ 200, /*cached*/ 100, /*output*/ 25),
        "turn",
    );
    let mut other = agent("other-session");
    other.update(
        usage(
            /*input*/ 99999, /*cached*/ 0, /*output*/ 99999,
        ),
        usage(
            /*input*/ 99999, /*cached*/ 0, /*output*/ 99999,
        ),
        "turn",
    );
    (
        SessionUsage {
            agents: BTreeMap::from([
                ("root".into(), parent),
                ("child".into(), child),
                ("nested".into(), nested),
                ("other".into(), other),
            ]),
            ..Default::default()
        },
        now,
    )
}

#[test]
fn dashboard_groups_descendants_without_other_sessions_or_double_counting_cache() {
    let (ledger, now) = dashboard();
    let lines = ledger.lines("root", /*detailed*/ false, now);
    let text = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("600 input · 300 cached · 75 output"));
    assert!(text.contains("1775 total"));
    assert!(!text.contains("other-session"));
    insta::assert_snapshot!("session_usage_summary", text);
    let detail = ledger.lines("root", /*detailed*/ true, now);
    insta::assert_snapshot!(
        "session_usage_details",
        detail
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
    for width in [42, 100] {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(
            width, /*height*/ 30,
        ))
        .expect("terminal");
        let wrapped = crate::wrapping::word_wrap_lines(detail.clone(), usize::from(width));
        terminal
            .draw(|frame| {
                use ratatui::widgets::Widget;
                ratatui::widgets::Paragraph::new(wrapped).render(frame.area(), frame.buffer_mut());
            })
            .expect("render");
        insta::assert_snapshot!(format!("session_usage_width_{width}"), terminal.backend());
    }
}

#[test]
fn missing_usage_and_incomplete_observation_are_visible() {
    let ledger = SessionUsage {
        incomplete: true,
        ..Default::default()
    };
    let text = ledger
        .lines("root", /*detailed*/ false, Instant::now())
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Waiting for usage"));
    assert!(text.contains("Partial data"));
}

#[tokio::test]
async fn app_tracks_background_usage_without_opening_dashboard() {
    let mut app = super::super::test_support::make_test_app().await;
    let mut server = crate::start_embedded_app_server_for_picker(app.chat_widget.config_ref())
        .await
        .expect("server");
    let started = server
        .start_thread(&app.config)
        .await
        .expect("start thread");
    let id = started.session.thread_id.to_string();
    let tokens = TokenUsageBreakdown {
        input_tokens: 100,
        cached_input_tokens: 50,
        output_tokens: 20,
        total_tokens: 120,
        cache_write_input_tokens: 0,
        reasoning_output_tokens: 5,
    };
    let event = ServerNotification::ThreadTokenUsageUpdated(ThreadTokenUsageUpdatedNotification {
        thread_id: id.clone(),
        turn_id: "turn".into(),
        token_usage: ThreadTokenUsage {
            total: tokens.clone(),
            last: tokens,
            model_context_window: None,
        },
    });
    for _ in 0..2 {
        app.handle_app_server_event(
            &server,
            codex_app_server_client::AppServerEvent::ServerNotification(Box::new(event.clone())),
        )
        .await;
    }
    assert_eq!(
        app.session_usage.lock().expect("usage lock").agents[&id].models,
        BTreeMap::from([(
            UNKNOWN.into(),
            Tokens {
                input: 100,
                cached: 50,
                output: 20,
                reasoning: 5,
                ..Default::default()
            }
        )])
    );
    assert!(app.overlay.is_none());
    tokio::time::timeout(Duration::from_secs(/*secs*/ 5), async {
        loop {
            if app.session_usage.lock().expect("usage lock").agents[&id].model
                == started.session.model
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await
    .expect("metadata resolves without a thread/started notification");
    assert!(!app.session_usage.lock().expect("usage lock").incomplete);
    server.shutdown().await.expect("shutdown");
}

#[test]
fn f5_is_available_and_yields_to_custom_bindings() {
    use crate::key_hint::KeyBindingListExt;
    let runtime = crate::keymap::RuntimeKeymap::defaults();
    assert!(
        runtime
            .app
            .open_session_usage
            .is_pressed(crossterm::event::KeyCode::F(5).into())
    );
    assert!(crate::slash_command::SlashCommand::DetailedStatus.available_during_task());
    for binding in ["f5", "f5 f12"] {
        let keymap: codex_config::types::TuiKeymap = serde_json::from_value(serde_json::json!({
            "editor": {"move_left": binding}
        }))
        .expect("config");
        let runtime = crate::keymap::RuntimeKeymap::from_config(&keymap).expect("custom binding");
        assert!(runtime.app.open_session_usage.is_empty());
    }
}
