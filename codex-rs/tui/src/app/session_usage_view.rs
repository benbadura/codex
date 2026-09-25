//! Live token dashboard backed by the UI ledger, with no server refresh loop.
#[cfg(test)]
#[path = "session_usage_view_tests.rs"]
mod tests;

use super::App;
use super::SessionUsage;
use super::Tokens;
use crate::pager_overlay::Overlay;
use crate::render::renderable::Renderable;
use crate::tui::FrameRequester;
use crate::tui::Tui;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

struct UsageView {
    usage: Arc<Mutex<SessionUsage>>,
    root: String,
    detailed: bool,
    frame_requester: FrameRequester,
}

impl SessionUsage {
    fn totals(&self, root: &str) -> (Tokens, Tokens) {
        let mut parent = Tokens::default();
        let mut children = Tokens::default();
        for (id, agent) in self.agents.iter().filter(|(id, _)| self.root(id) == root) {
            for tokens in agent.models.values() {
                if id == root {
                    parent.add(*tokens);
                } else {
                    children.add(*tokens);
                }
            }
        }
        (parent, children)
    }

    pub(in crate::app) fn compact_line(&self, root: &str, toggle_key: &str) -> Line<'static> {
        let (parent, children) = self.totals(root);
        let mut total = parent;
        total.add(children);
        let total = total.input.saturating_add(total.output);
        let parent = parent.input.saturating_add(parent.output);
        let children = children.input.saturating_add(children.output);
        vec![
            "Tokens ".bold(),
            format!("{} total", crate::status::format_tokens_compact(total)).bold(),
            " · parent ".dim(),
            crate::status::format_tokens_compact(parent).into(),
            " · subagents ".dim(),
            crate::status::format_tokens_compact(children).into(),
            format!(" · {toggle_key} hide · /detailed-status").dim(),
        ]
        .into()
    }

    pub(super) fn lines(&self, root: &str, detailed: bool, now: Instant) -> Vec<Line<'static>> {
        let (parent, children) = self.totals(root);
        let mut models: BTreeMap<(&str, &str), Tokens> = BTreeMap::new();
        let agents: Vec<_> = self
            .agents
            .iter()
            .filter(|(id, _)| self.root(id) == root)
            .collect();
        for (id, agent) in &agents {
            let role = if id.as_str() == root {
                "Parent"
            } else {
                "Subagents"
            };
            for (model, tokens) in &agent.models {
                models.entry((role, model)).or_default().add(*tokens);
            }
        }
        let mut total = parent;
        total.add(children);
        let mut lines = vec![
            "Session usage · live".bold().into(),
            format!("Root: {root}").dim().into(),
            "Tokens, not a bill. Monetary cost unavailable without verified pricing."
                .dim()
                .into(),
            "Observed threads only; updates arrive after model responses, not per token."
                .dim()
                .into(),
            "".into(),
            format!("Parent     {}", token_text(parent)).into(),
            format!("Subagents  {}", token_text(children)).into(),
            format!("Total      {}", token_text(total)).bold().into(),
            "".into(),
            "Usage by model".bold().into(),
        ];
        if models.is_empty() {
            lines.push("Waiting for usage notifications…".dim().into());
        }
        for ((role, model), tokens) in models {
            lines.push(format!("{role} · {model}").into());
            lines.push(format!("  {}", token_text(tokens)).into());
        }
        if detailed {
            lines.push("".into());
            lines.push("Agents · completed agents remain included".bold().into());
            for (id, agent) in agents {
                let role = if id == root { "Parent" } else { "Agent" };
                lines.push(
                    format!(
                        "{role}: {}",
                        if agent.name.is_empty() {
                            id
                        } else {
                            &agent.name
                        }
                    )
                    .bold()
                    .into(),
                );
                lines.push(
                    format!(
                        "  ID {id} · parent {}",
                        agent.parent.as_deref().unwrap_or("—")
                    )
                    .dim()
                    .into(),
                );
                lines.push(
                    format!(
                        "  {} · current model {} · reasoning {} · {} observed turns · active time {:.1}s",
                        agent.status,
                        agent.model,
                        agent
                            .reasoning_effort
                            .as_ref()
                            .map(ToString::to_string)
                            .as_deref()
                            .unwrap_or("unknown"),
                        agent.turns.len(),
                        agent.elapsed_ms(now) as f64 / 1000.0
                    )
                    .into(),
                );
                if agent.previous.is_none() {
                    lines.push("  Usage not yet reported".dim().into());
                }
                for (model, tokens) in &agent.models {
                    lines.push(format!("  {model}: {}", token_text(*tokens)).into());
                    lines.push(format!("    reasoning {} (included in output) · cache writes {} (provider counter)", tokens.reasoning, tokens.write).dim().into());
                }
            }
            lines.push("".into());
            lines.push("Active time sums observed turns, including tool execution and waiting; it is not inference time.".dim().into());
        } else {
            lines.push("".into());
            lines.push(
                "Close and run /detailed-status for per-agent models, tokens and active time."
                    .dim()
                    .into(),
            );
        }
        lines.push("Input includes cached input; total = input + output. Cache and reasoning are not added twice.".dim().into());
        lines.push("Unknown = history/model attribution unavailable. Closed agents are retained until this TUI exits.".dim().into());
        if self.incomplete {
            lines.push(Line::from("Some agent details may be incomplete because metadata could not be loaded, history changed, or the tracking limit was reached.").style(crate::style::status_style(crate::style::StatusTone::Attention)));
        }
        lines
    }
}

fn token_text(tokens: Tokens) -> String {
    format!(
        "{} total · {} input · {} cached · {} output",
        tokens.input.saturating_add(tokens.output),
        tokens.input,
        tokens.cached,
        tokens.output
    )
}

impl UsageView {
    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let usage = self
            .usage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::wrapping::word_wrap_lines(
            usage.lines(&usage.root(&self.root), self.detailed, Instant::now()),
            usize::from(width.max(1)),
        )
    }
}

impl Renderable for UsageView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.lines(area.width)).render(area, buf);
        self.frame_requester
            .schedule_frame_in(Duration::from_secs(/*secs*/ 1));
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.lines(width).len().min(u16::MAX as usize) as u16
    }
}

impl App {
    pub(in crate::app) fn toggle_session_usage_live(&mut self, tui: &Tui) {
        self.session_usage_live_visible = !self.session_usage_live_visible;
        tui.frame_requester().schedule_frame();
    }

    pub(in crate::app) fn open_session_usage(
        &mut self,
        tui: &mut Tui,
        detailed: bool,
    ) -> std::io::Result<()> {
        let Some(id) = self.chat_widget.thread_id() else {
            return Ok(());
        };
        let root = {
            let mut usage = self
                .session_usage
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for thread in self.agents_overview.threads.values().flatten() {
                if !usage.agents.contains_key(&thread.id) {
                    usage.metadata(thread);
                }
            }
            let agent = usage.agents.entry(id.to_string()).or_default();
            if agent.model.is_empty() {
                agent.model = self.chat_widget.current_model().to_string();
            }
            if agent.reasoning_effort.is_none() {
                agent.reasoning_effort = self.chat_widget.current_reasoning_effort();
            }
            if agent.previous.is_none() && !agent.reset {
                let tokens = self.chat_widget.token_usage();
                let total = Tokens {
                    input: tokens.input_tokens,
                    cached: tokens.cached_input_tokens,
                    output: tokens.output_tokens,
                    reasoning: tokens.reasoning_output_tokens,
                    write: 0,
                };
                if total != Tokens::default() {
                    agent.update(total, Tokens::default(), "");
                }
            }
            usage.root(&id.to_string())
        };
        let mut keymap = self.keymap.pager.clone();
        keymap
            .close
            .extend(self.keymap.app.open_session_usage.iter().copied());
        tui.enter_alt_screen()?;
        self.overlay = Some(Overlay::new_static_with_renderables(
            vec![Box::new(UsageView {
                usage: Arc::clone(&self.session_usage),
                root,
                detailed,
                frame_requester: tui.frame_requester(),
            })],
            if detailed {
                "Detailed session usage"
            } else {
                "Session usage"
            }
            .to_string(),
            keymap,
        ));
        tui.frame_requester().schedule_frame();
        Ok(())
    }
}
