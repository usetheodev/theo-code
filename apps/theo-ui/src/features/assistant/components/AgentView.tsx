import { useRef, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { motion, AnimatePresence } from "framer-motion";
import { Sparkles, Shield, FormInput, GitBranch, ArrowRight, Settings } from "lucide-react";
import { ChatMessage } from "../../../types";
import { ToolCallDisplay } from "./ToolCallDisplay";
import { AssistantMessage } from "./AssistantMessage";

const EASE_OUT = [0.22, 1, 0.36, 1] as const;

interface AgentViewProps {
  messages: ChatMessage[];
  isRunning: boolean;
}

export function AgentView({ messages, isRunning }: AgentViewProps) {
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  if (messages.length === 0) {
    return <EmptyState />;
  }

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="max-w-[720px] mx-auto px-6 py-8 flex flex-col gap-5">
        <AnimatePresence initial={false}>
          {messages.map((msg) => {
            if (msg.role === "tool" && msg.toolName) {
              return (
                <motion.div
                  key={msg.id}
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ duration: 0.18, ease: EASE_OUT }}
                >
                  <ToolCallDisplay msg={msg} />
                </motion.div>
              );
            }

            if (msg.role === "user") {
              return (
                <motion.div
                  key={msg.id}
                  className="flex justify-end"
                  initial={{ opacity: 0, y: 6, scale: 0.98 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  transition={{ duration: 0.2, ease: EASE_OUT }}
                >
                  <div className="max-w-[80%] px-4 py-2.5 rounded-2xl rounded-br-md bg-brand text-white text-[14px] leading-relaxed shadow-sm shadow-brand/10">
                    {msg.content}
                  </div>
                </motion.div>
              );
            }

            const isLastMsg = msg === messages[messages.length - 1];
            return (
              <motion.div
                key={msg.id}
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.22, ease: EASE_OUT }}
              >
                <AssistantMessage
                  content={msg.content}
                  reasoning={msg.reasoning}
                  isStreaming={isRunning && isLastMsg}
                  isDone={msg.done}
                />
              </motion.div>
            );
          })}
        </AnimatePresence>

        {/* Thinking indicator */}
        <AnimatePresence>
          {isRunning && messages[messages.length - 1]?.role !== "assistant" && (
            <ThinkingIndicator />
          )}
        </AnimatePresence>

        <div ref={endRef} />
      </div>
    </div>
  );
}

function ThinkingIndicator() {
  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -4 }}
      transition={{ duration: 0.2, ease: EASE_OUT }}
      className="flex items-center gap-3 py-3"
    >
      <div className="relative flex items-center justify-center w-7 h-7">
        <div className="absolute inset-0 rounded-full bg-brand/10 animate-ping" style={{ animationDuration: "2s" }} />
        <div className="relative w-7 h-7 rounded-full bg-surface-2 ring-1 ring-brand/20 flex items-center justify-center">
          <Sparkles size={13} className="text-brand" />
        </div>
      </div>
      <span className="text-[13px] text-text-2">Thinking...</span>
    </motion.div>
  );
}

const SUGGESTIONS = [
  { text: "Review my authentication flow for security issues", icon: Shield },
  { text: "Add input validation to the user registration form", icon: FormInput },
  { text: "Plan a migration from REST to GraphQL", icon: GitBranch },
];

function EmptyState() {
  const navigate = useNavigate();

  return (
    <div className="flex-1 flex items-center justify-center px-6">
      <div className="flex flex-col items-center gap-12 max-w-xl w-full text-center">
        {/* Hero */}
        <motion.div
          className="flex flex-col items-center gap-6"
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.4, ease: EASE_OUT }}
        >
          <div className="relative">
            <motion.div
              className="w-[72px] h-[72px] rounded-[20px] bg-gradient-to-br from-brand/20 via-brand/10 to-transparent flex items-center justify-center ring-1 ring-brand/15"
              animate={{ scale: [1, 1.02, 1] }}
              transition={{ duration: 4, repeat: Infinity, ease: "easeInOut" }}
            >
              <Sparkles size={30} className="text-brand" />
            </motion.div>
            <div className="absolute -inset-4 rounded-[28px] bg-brand/[0.04] blur-2xl -z-10" />
          </div>
          <div className="space-y-3">
            <h2 className="text-[28px] font-semibold text-text-0 tracking-tight leading-tight">
              What can I help you build?
            </h2>
            <p className="text-[15px] text-text-2 leading-relaxed max-w-sm mx-auto">
              Describe a task, ask for a review, or plan your next feature.
            </p>
          </div>
        </motion.div>

        {/* Suggestions — staggered */}
        <div className="flex flex-col gap-3 w-full">
          {SUGGESTIONS.map(({ text, icon: Icon }, i) => (
            <motion.button
              key={text}
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.3, delay: 0.15 + i * 0.08, ease: EASE_OUT }}
              whileHover={{ y: -1, transition: { duration: 0.15 } }}
              whileTap={{ scale: 0.985, transition: { duration: 0.08 } }}
              className="flex items-center gap-4 px-5 py-4 rounded-xl bg-surface-1 border border-white/[0.06] text-left text-[13px] text-text-1 hover:bg-surface-2 hover:border-white/[0.12] hover:shadow-[0_8px_24px_rgba(0,0,0,0.25)] transition-[background,border,box-shadow] duration-200 group cursor-pointer"
            >
              <div className="flex items-center justify-center w-9 h-9 rounded-lg bg-white/[0.03] ring-1 ring-white/[0.06] shrink-0 group-hover:bg-brand/10 group-hover:ring-brand/20 transition-all">
                <Icon size={16} className="text-text-3 group-hover:text-brand transition-colors" />
              </div>
              <span className="flex-1 leading-relaxed">{text}</span>
              <ArrowRight size={14} className="text-text-3/0 group-hover:text-text-2 transition-all shrink-0" />
            </motion.button>
          ))}
        </div>

        {/* Settings */}
        <motion.button
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.3, delay: 0.5 }}
          onClick={() => navigate("/settings")}
          className="flex items-center gap-2 px-3 py-1.5 rounded-lg text-[12px] text-text-3 hover:text-text-1 hover:bg-white/[0.03] transition-all"
        >
          <Settings size={13} />
          Configure provider and project
        </motion.button>
      </div>
    </div>
  );
}
