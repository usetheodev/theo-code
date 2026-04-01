export type AgentEventType =
  | { type: "token"; text: string }
  | { type: "tool_start"; name: string; args: Record<string, unknown> }
  | { type: "tool_end"; name: string; success: boolean; output: string }
  | { type: "phase_change"; from: string; to: string }
  | { type: "done"; success: boolean; summary: string }
  | { type: "error"; message: string }
  | { type: "llm_call_start"; iteration: number }
  | { type: "llm_call_end"; iteration: number };

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "tool";
  content: string;
  reasoning?: string;
  toolName?: string;
  toolSuccess?: boolean;
  toolArgs?: Record<string, unknown>;
  collapsed?: boolean;
  done?: boolean;
}

export interface AppConfig {
  base_url: string;
  model: string;
  has_api_key: boolean;
  max_iterations: number;
  temperature: number;
}

export interface AuthStatus {
  authenticated: boolean;
  expired: boolean;
  account_id?: string;
  has_refresh_token?: boolean;
}

export type AgentMode = "plan" | "review" | "edit";

export interface AppLayoutContext {
  config: AppConfig | null;
  authStatus: AuthStatus | null;
  projectDir: string | null;
  reloadState: () => Promise<void>;
}
