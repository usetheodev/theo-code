use std::path::PathBuf;
use tokio::sync::{Mutex, watch};

use theo_agent_runtime::AgentConfig;

/// Application state shared across Tauri commands.
pub struct AppState {
    pub project_dir: Mutex<Option<PathBuf>>,
    pub config: Mutex<AgentConfig>,
    /// Signal to cancel the running agent.
    pub cancel_tx: Mutex<Option<watch::Sender<bool>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            project_dir: Mutex::new(None),
            config: Mutex::new(AgentConfig::default()),
            cancel_tx: Mutex::new(None),
        }
    }
}
