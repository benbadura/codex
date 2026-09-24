use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn session_usage_opens_for_selected_thread_and_f5_closes_it() {
    let mut app = crate::app::test_support::make_test_app().await;
    let mut tui = crate::tui::test_support::make_test_tui().expect("tui");
    app.chat_widget
        .handle_thread_session_quiet(crate::session_state::ThreadSessionState {
            windows_sandbox_host: crate::app::WindowsSandboxHost::Local,
            thread_id: codex_protocol::ThreadId::new(),
            forked_from_id: None,
            fork_parent_title: None,
            thread_name: None,
            model: "astra".into(),
            model_provider_id: "openai".into(),
            service_tier: None,
            approval_policy: codex_app_server_protocol::AskForApproval::Never,
            approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer::User,
            permission_profile: codex_protocol::models::PermissionProfile::read_only(),
            active_permission_profile: None,
            cwd: app.config.cwd.clone(),
            runtime_workspace_roots: Vec::new(),
            instruction_source_paths: Vec::new(),
            reasoning_effort: None,
            collaboration_mode: None,
            personality: None,
            message_history: None,
            network_proxy: None,
            rollout_path: None,
        });
    app.open_session_usage(&mut tui, /*detailed*/ true)
        .expect("open");
    let overlay = app.overlay.as_mut().expect("usage overlay");
    assert!(!overlay.is_done());
    overlay
        .handle_event(
            &mut tui,
            crate::tui::TuiEvent::Key(crossterm::event::KeyCode::F(5).into()),
        )
        .expect("close");
    assert!(overlay.is_done());
}

#[tokio::test]
async fn live_view_reads_new_usage_and_grows_without_reopening() {
    let tui = crate::tui::test_support::make_test_tui().expect("tui");
    let usage = Arc::new(Mutex::new(SessionUsage::default()));
    let view = UsageView {
        usage: Arc::clone(&usage),
        root: "parent".into(),
        detailed: true,
        frame_requester: tui.frame_requester(),
    };
    let before = view.desired_height(/*width*/ 100);
    {
        let mut ledger = usage.lock().expect("usage lock");
        let agent = ledger.agents.entry("parent".into()).or_default();
        agent.name = "Coordinator".into();
        agent.model = "astra".into();
        agent.status = "Completed".into();
        agent.previous = Some(Tokens {
            input: 100,
            output: 10,
            ..Default::default()
        });
        agent.record("astra".into(), agent.previous.expect("baseline"));
    }
    let after = view.desired_height(/*width*/ 100);
    assert!(after > before);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(
        /*width*/ 100, /*height*/ 30,
    ))
    .expect("terminal");
    terminal
        .draw(|frame| view.render(frame.area(), frame.buffer_mut()))
        .expect("render");
    insta::assert_snapshot!("session_usage_live_update", terminal.backend());
    assert_eq!(view.desired_height(/*width*/ 100), after);
}
