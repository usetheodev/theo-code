use std::path::Path;
use std::sync::Arc;

use theo_domain::session::{MessageId, SessionId};
use theo_domain::tool::ToolContext;
use theo_infra_llm::types::{ChatRequest, Message};
use theo_infra_llm::LlmClient;
use theo_tooling::registry::ToolRegistry;

use crate::config::AgentConfig;
use crate::events::{AgentEvent, EventSink};
use crate::state::{AgentState, Phase};
use crate::tool_bridge;

/// Result of an agent loop execution.
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub success: bool,
    pub summary: String,
    pub files_edited: Vec<String>,
    pub iterations_used: usize,
}

/// The main agent loop that orchestrates LLM ↔ tool execution.
pub struct AgentLoop {
    client: LlmClient,
    registry: ToolRegistry,
    config: AgentConfig,
    event_sink: Arc<dyn EventSink>,
}

impl AgentLoop {
    pub fn new(
        config: AgentConfig,
        registry: ToolRegistry,
        event_sink: Arc<dyn EventSink>,
    ) -> Self {
        let mut client = LlmClient::new(
            &config.base_url,
            config.api_key.clone(),
            &config.model,
        );
        if let Some(ref endpoint) = config.endpoint_override {
            client = client.with_endpoint(endpoint);
        }
        for (k, v) in &config.extra_headers {
            client = client.with_header(k, v);
        }
        Self {
            client,
            registry,
            config,
            event_sink,
        }
    }

    /// Run the agent loop on a task.
    pub async fn run(&self, task: &str, project_dir: &Path) -> AgentResult {
        let mut state = AgentState::new();
        let mut messages: Vec<Message> = vec![
            Message::system(&self.config.system_prompt),
            Message::user(task),
        ];

        let tool_defs = tool_bridge::registry_to_definitions(&self.registry);
        let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);

        for iteration in 1..=self.config.max_iterations {
            // Context loop injection
            if iteration > 1 && iteration % self.config.context_loop_interval == 0 {
                let ctx_msg = state.build_context_loop(iteration, self.config.max_iterations, task);
                self.event_sink.emit(AgentEvent::ContextLoop {
                    iteration,
                    message: ctx_msg.clone(),
                });
                messages.push(Message::user(ctx_msg));
            }

            // Phase transitions
            let old_phase = state.phase;
            state.maybe_transition(iteration, self.config.max_iterations);
            if state.phase != old_phase {
                self.event_sink.emit(AgentEvent::PhaseChange {
                    from: old_phase,
                    to: state.phase,
                });
            }

            // Phase-specific nudges
            if let Some(nudge) = phase_nudge(&state, iteration, self.config.max_iterations) {
                messages.push(Message::user(nudge));
            }

            // LLM call
            self.event_sink.emit(AgentEvent::LlmCallStart { iteration });

            let request = ChatRequest::new(&self.config.model, messages.clone())
                .with_tools(tool_defs.clone())
                .with_max_tokens(self.config.max_tokens)
                .with_temperature(self.config.temperature);

            let response = match self.client.chat(&request).await {
                Ok(resp) => resp,
                Err(e) => {
                    self.event_sink.emit(AgentEvent::Error(format!("LLM error: {e}")));
                    // Retry once on transient errors
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    match self.client.chat(&request).await {
                        Ok(resp) => resp,
                        Err(e) => {
                            self.event_sink.emit(AgentEvent::Error(format!("LLM error (retry failed): {e}")));
                            break;
                        }
                    }
                }
            };

            self.event_sink.emit(AgentEvent::LlmCallEnd { iteration });

            // Process content
            if let Some(content) = response.content() {
                if !content.is_empty() {
                    self.event_sink.emit(AgentEvent::Token(content.to_string()));
                }
            }

            let tool_calls = response.tool_calls();

            // No tool calls → assistant text response, append and continue
            if tool_calls.is_empty() {
                let content = response.content().unwrap_or("").to_string();
                messages.push(Message::assistant(content));
                continue;
            }

            // Append assistant message with tool calls
            messages.push(Message::assistant_with_tool_calls(
                response.content().map(String::from),
                tool_calls.to_vec(),
            ));

            // Execute each tool call
            for call in tool_calls {
                let name = &call.function.name;

                // Handle `done` meta-tool
                if name == "done" {
                    let summary = call
                        .parse_arguments()
                        .ok()
                        .and_then(|args| args.get("summary").and_then(|s| s.as_str()).map(String::from))
                        .unwrap_or_else(|| "Task completed.".to_string());

                    // Promise gate: check if there are real changes
                    if has_real_changes(project_dir).await {
                        self.event_sink.emit(AgentEvent::Done {
                            success: true,
                            summary: summary.clone(),
                        });
                        return AgentResult {
                            success: true,
                            summary,
                            files_edited: state.edits_files.clone(),
                            iterations_used: iteration,
                        };
                    } else {
                        state.record_done_blocked();
                        self.event_sink.emit(AgentEvent::Error(
                            "done() blocked: no real changes detected in git diff. Keep working.".to_string(),
                        ));
                        messages.push(Message::tool_result(
                            &call.id,
                            "done",
                            "BLOCKED: No real changes detected (git diff is empty). You must make actual code changes before calling done(). Re-read the task and try again.",
                        ));
                        continue;
                    }
                }

                // Execute regular tool
                self.event_sink.emit(AgentEvent::ToolStart {
                    name: name.clone(),
                    args: call.parse_arguments().unwrap_or_default(),
                });

                let ctx = ToolContext {
                    session_id: SessionId::new("agent"),
                    message_id: MessageId::new(&format!("iter_{iteration}")),
                    call_id: call.id.clone(),
                    agent: "main".to_string(),
                    abort: abort_rx.clone(),
                    project_dir: project_dir.to_path_buf(),
                };

                let (result_msg, success) =
                    tool_bridge::execute_tool_call(&self.registry, call, &ctx).await;

                let output = result_msg.content.clone().unwrap_or_default();
                self.event_sink.emit(AgentEvent::ToolEnd {
                    name: name.clone(),
                    success,
                    output: output.clone(),
                });

                // Update state based on tool
                match name.as_str() {
                    "read" => {
                        if let Ok(args) = call.parse_arguments() {
                            if let Some(path) = args.get("filePath").and_then(|p| p.as_str()) {
                                state.record_read(path);
                            }
                        }
                    }
                    "grep" | "glob" => state.record_search(),
                    "edit" | "write" | "apply_patch" => {
                        let file = call
                            .parse_arguments()
                            .ok()
                            .and_then(|args| {
                                args.get("filePath")
                                    .or(args.get("file_path"))
                                    .and_then(|p| p.as_str())
                                    .map(String::from)
                            })
                            .unwrap_or_default();
                        state.record_edit_attempt(&file, success, if success { None } else { Some(output.clone()) });
                    }
                    _ => {}
                }

                messages.push(result_msg);
            }
        }

        // Max iterations reached
        let _ = abort_tx.send(true);
        let summary = format!(
            "Max iterations ({}) reached. Edits succeeded: {}. Files: {}",
            self.config.max_iterations,
            state.edits_succeeded,
            state.edits_files.join(", ")
        );

        self.event_sink.emit(AgentEvent::Done {
            success: state.edits_succeeded > 0,
            summary: summary.clone(),
        });

        AgentResult {
            success: state.edits_succeeded > 0,
            summary,
            files_edited: state.edits_files,
            iterations_used: self.config.max_iterations,
        }
    }
}

/// Check if the project has real uncommitted changes via git diff.
async fn has_real_changes(project_dir: &Path) -> bool {
    let output = tokio::process::Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(project_dir)
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            !stdout.trim().is_empty()
        }
        Err(_) => true, // If git fails, assume changes exist
    }
}

/// Generate a phase-specific nudge message if appropriate.
fn phase_nudge(state: &AgentState, iteration: usize, max_iterations: usize) -> Option<String> {
    let two_thirds = (max_iterations * 2) / 3;

    match state.phase {
        Phase::Edit if iteration >= two_thirds && state.edits_succeeded == 0 => {
            Some("URGENT: You have very few iterations left and NO successful edits. Stop reading/searching and EDIT a file NOW.".to_string())
        }
        Phase::Edit if state.edit_attempts > 3 && state.edits_succeeded == 0 => {
            Some("Your edits keep failing. Read the target file again carefully, then try a different edit approach.".to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_result_default() {
        let result = AgentResult {
            success: false,
            summary: "test".to_string(),
            files_edited: vec![],
            iterations_used: 0,
        };
        assert!(!result.success);
    }

    #[test]
    fn test_phase_nudge_urgent() {
        let mut state = AgentState::new();
        state.phase = Phase::Edit;
        let nudge = phase_nudge(&state, 10, 15);
        assert!(nudge.is_some());
        assert!(nudge.unwrap().contains("URGENT"));
    }

    #[test]
    fn test_phase_nudge_none_in_explore() {
        let state = AgentState::new();
        let nudge = phase_nudge(&state, 1, 15);
        assert!(nudge.is_none());
    }
}
