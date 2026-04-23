//! T1.6 integration test — verifies that the observability pipeline produces
//! a trajectory JSONL under `<project_dir>/.theo/trajectories/` for a fresh run.

use std::sync::Arc;

use theo_agent_runtime::event_bus::EventBus;
use theo_agent_runtime::run_engine::AgentRunEngine;
use theo_agent_runtime::task_manager::TaskManager;
use theo_agent_runtime::tool_call_manager::ToolCallManager;
use theo_agent_runtime::AgentConfig;
use theo_domain::session::SessionId;
use theo_domain::task::AgentType;
use theo_infra_llm::client::LlmClient;

#[test]
fn trajectory_file_is_created_for_run() {
    let tmp = tempfile::tempdir().unwrap();
    let bus = Arc::new(EventBus::new());
    let tm = Arc::new(TaskManager::new(bus.clone()));
    let tcm = Arc::new(ToolCallManager::new(bus.clone()));
    let task_id = tm.create_task(SessionId::new("s"), AgentType::Coder, "test".into());
    let engine = AgentRunEngine::new(
        task_id,
        tm,
        tcm,
        bus,
        LlmClient::new("http://localhost:9999", None, "test"),
        Arc::new(theo_tooling::registry::create_default_registry()),
        AgentConfig::default(),
        tmp.path().to_path_buf(),
    );
    let run_id = engine.run_id().as_str().to_string();
    drop(engine);
    std::thread::sleep(std::time::Duration::from_millis(150));
    let expected = tmp
        .path()
        .join(".theo")
        .join("trajectories")
        .join(format!("{}.jsonl", run_id));
    assert!(expected.exists(), "trajectory JSONL must exist: {:?}", expected);
}
