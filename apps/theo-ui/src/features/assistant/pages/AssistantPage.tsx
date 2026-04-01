import { useState, useCallback } from "react";
import { useOutletContext } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { motion, AnimatePresence } from "framer-motion";
import { cn } from "@/lib/utils";
import { Bot, TestTube2, ScanSearch, ShieldCheck, ListChecks } from "lucide-react";
import { AgentView } from "../components/AgentView";
import { AgentTestsView } from "../components/AgentTestsView";
import { AgentReviewView } from "../components/AgentReviewView";
import { AgentSecurityView } from "../components/AgentSecurityView";
import { AgentPlanView } from "../components/AgentPlanView";
import { CommandComposer } from "../components/CommandComposer";
import { useAgentEvents } from "../../../hooks/useAgentEvents";
import { ChatMessage, AgentMode, AppLayoutContext } from "../../../types";

const TABS = [
  { id: "agent", label: "Agent", icon: Bot },
  { id: "plan", label: "Agent Plan", icon: ListChecks },
  { id: "tests", label: "Agent Tests", icon: TestTube2 },
  { id: "review", label: "Agent Review", icon: ScanSearch },
  { id: "security", label: "Agent Security", icon: ShieldCheck },
] as const;

export function AssistantPage() {
  const { config, projectDir } = useOutletContext<AppLayoutContext>();
  const [activeTab, setActiveTab] = useState("agent");
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isRunning, setIsRunning] = useState(false);

  useAgentEvents(setMessages, setIsRunning);

  const handleSubmit = useCallback(async (text: string, mode: AgentMode) => {
    if (!projectDir) return;

    const modePrefix: Record<AgentMode, string> = {
      plan: "[MODE: PLAN — Generate a plan only, do NOT execute changes]\n\n",
      review: "[MODE: REVIEW — Analyze and review the code, suggest improvements]\n\n",
      edit: "",
    };

    const fullMessage = modePrefix[mode] + text;

    setMessages((prev) => [
      ...prev,
      { id: `msg_user_${Date.now()}`, role: "user", content: text },
    ]);
    setIsRunning(true);

    try {
      await invoke("send_message", { message: fullMessage });
    } catch (e: any) {
      setIsRunning(false);
      const errorMsg = typeof e === "string" ? e : e?.message || JSON.stringify(e);
      setMessages((prev) => [
        ...prev,
        { id: `msg_err_${Date.now()}`, role: "assistant", content: `Error: ${errorMsg}` },
      ]);
    }
  }, [projectDir]);

  const handleCancel = useCallback(async () => {
    try {
      await invoke("cancel_agent");
      setIsRunning(false);
    } catch (e) {
      console.error("Cancel failed:", e);
    }
  }, []);

  const isReady = !!projectDir && !!config?.has_api_key;
  const disabledReason = !projectDir
    ? "Select a project directory in Settings"
    : !config?.has_api_key
      ? "Add an API key in Settings"
      : undefined;

  return (
    <div className="flex flex-col h-full overflow-hidden bg-surface-0">
      {/* Tabs */}
      <div className="flex items-center gap-0.5 px-5 h-[48px] border-b border-white/[0.05] shrink-0 overflow-x-auto">
        {TABS.map((tab) => {
          const Icon = tab.icon;
          const isActive = activeTab === tab.id;
          return (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={cn(
                "relative flex items-center gap-2 px-4 h-[48px] text-[13px] -mb-px transition-colors duration-150 whitespace-nowrap shrink-0",
                isActive
                  ? "text-text-0 font-medium"
                  : "text-text-3 hover:text-text-1 hover:bg-white/[0.02]",
              )}
            >
              <Icon size={15} className={cn("transition-colors duration-150", isActive ? "text-brand" : "")} />
              {tab.label}
              {isActive && (
                <motion.span
                  layoutId="tab-underline"
                  className="absolute bottom-0 left-4 right-4 h-[2px] rounded-full bg-brand"
                  transition={{ type: "spring", stiffness: 400, damping: 30 }}
                />
              )}
            </button>
          );
        })}
      </div>

      {/* Content with crossfade */}
      <AnimatePresence mode="wait">
        <motion.div
          key={activeTab}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          className="flex-1 flex flex-col overflow-hidden"
        >
          {activeTab === "agent" && <AgentView messages={messages} isRunning={isRunning} />}
          {activeTab === "plan" && <AgentPlanView />}
          {activeTab === "tests" && <AgentTestsView />}
          {activeTab === "review" && <AgentReviewView />}
          {activeTab === "security" && <AgentSecurityView />}
        </motion.div>
      </AnimatePresence>

      {/* Composer */}
      <CommandComposer
        onSubmit={handleSubmit}
        isRunning={isRunning}
        onCancel={handleCancel}
        disabled={!isReady}
        disabledReason={disabledReason}
      />
    </div>
  );
}
