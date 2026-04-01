import { useState, useCallback, useRef, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { cn } from "@/lib/utils";
import { ArrowUp, Square, ChevronDown, Pencil, FileSearch, ListChecks, Loader2 } from "lucide-react";
import { AgentMode } from "../../../types";

interface CommandComposerProps {
  onSubmit: (text: string, mode: AgentMode) => void;
  isRunning: boolean;
  onCancel: () => void;
  disabled?: boolean;
  disabledReason?: string;
}

const MODE_CONFIG: Record<AgentMode, { label: string; placeholder: string; color: string; dotColor: string; icon: React.ComponentType<any> }> = {
  edit: {
    label: "Edit mode",
    placeholder: "Ask the agent to make changes to your code...",
    color: "text-ok",
    dotColor: "bg-ok",
    icon: Pencil,
  },
  plan: {
    label: "Plan mode",
    placeholder: "Describe what you want — the agent will plan without executing...",
    color: "text-info",
    dotColor: "bg-info",
    icon: ListChecks,
  },
  review: {
    label: "Review mode",
    placeholder: "What should the agent review or analyze?",
    color: "text-warn",
    dotColor: "bg-warn",
    icon: FileSearch,
  },
};

const MODES: AgentMode[] = ["edit", "plan", "review"];
const EASE_OUT = [0.22, 1, 0.36, 1] as const;

export function CommandComposer({ onSubmit, isRunning, onCancel, disabled, disabledReason }: CommandComposerProps) {
  const [mode, setMode] = useState<AgentMode>("edit");
  const [text, setText] = useState("");
  const [showMenu, setShowMenu] = useState(false);
  const [isFocused, setIsFocused] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const config = MODE_CONFIG[mode];
  const canSubmit = text.trim().length > 0 && !isRunning && !disabled;

  const handleSubmit = useCallback(() => {
    const trimmed = text.trim();
    if (!trimmed || isRunning || disabled) return;
    onSubmit(trimmed, mode);
    setText("");
    if (textareaRef.current) textareaRef.current.style.height = "auto";
  }, [text, mode, isRunning, disabled, onSubmit]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  useEffect(() => {
    const ta = textareaRef.current;
    if (ta) {
      ta.style.height = "auto";
      ta.style.height = Math.min(ta.scrollHeight, 200) + "px";
    }
  }, [text]);

  useEffect(() => {
    if (!showMenu) return;
    const handler = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setShowMenu(false);
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [showMenu]);

  return (
    <div className="px-6 pb-6 pt-3">
      <div className="max-w-[720px] mx-auto">
        <motion.div
          animate={isFocused && !disabled ? "focused" : "idle"}
          variants={{
            idle: {
              boxShadow: "0 4px 20px rgba(0,0,0,0.25), 0 0 0 0px rgba(108,92,231,0)",
              borderColor: "rgba(255,255,255,0.1)",
            },
            focused: {
              boxShadow: "0 8px 40px rgba(0,0,0,0.35), 0 0 0 1px rgba(108,92,231,0.15), 0 2px 12px rgba(108,92,231,0.08)",
              borderColor: "rgba(108,92,231,0.4)",
            },
          }}
          transition={{ duration: 0.2, ease: EASE_OUT }}
          className={cn(
            "relative rounded-2xl border bg-surface-2",
            disabled && "opacity-50",
          )}
        >
          {/* Mode bar */}
          <div className="flex items-center px-5 pt-3.5 pb-0">
            <div className="relative" ref={menuRef}>
              <motion.button
                onClick={() => setShowMenu(!showMenu)}
                disabled={disabled || isRunning}
                whileTap={{ scale: 0.97 }}
                className={cn(
                  "flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-[11px] font-semibold tracking-wide transition-colors",
                  "bg-white/[0.04] hover:bg-white/[0.07]",
                  config.color,
                  "disabled:opacity-40",
                )}
              >
                <motion.span
                  key={mode}
                  initial={{ scale: 0.5, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  transition={{ duration: 0.15 }}
                  className={cn("size-2 rounded-full", config.dotColor)}
                />
                <AnimatePresence mode="wait">
                  <motion.span
                    key={mode}
                    initial={{ opacity: 0, y: 4 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -4 }}
                    transition={{ duration: 0.12 }}
                  >
                    {config.label}
                  </motion.span>
                </AnimatePresence>
                <ChevronDown size={10} className="opacity-50 ml-0.5" />
              </motion.button>

              <AnimatePresence>
                {showMenu && (
                  <motion.div
                    initial={{ opacity: 0, y: -4, scale: 0.97 }}
                    animate={{ opacity: 1, y: 0, scale: 1 }}
                    exit={{ opacity: 0, y: -4, scale: 0.97 }}
                    transition={{ duration: 0.15, ease: EASE_OUT }}
                    className="absolute top-full left-0 mt-1.5 bg-surface-3 border border-white/[0.1] rounded-xl py-1.5 z-20 min-w-[150px] shadow-2xl shadow-black/50"
                  >
                    {MODES.map((m) => {
                      const mc = MODE_CONFIG[m];
                      const Icon = mc.icon;
                      return (
                        <button
                          key={m}
                          onClick={() => { setMode(m); setShowMenu(false); }}
                          className={cn(
                            "w-full flex items-center gap-2.5 px-3 py-2 text-[12px] font-medium transition-colors",
                            m === mode
                              ? cn(mc.color, "bg-white/[0.06]")
                              : "text-text-1 hover:bg-white/[0.04]",
                          )}
                        >
                          <Icon size={13} />
                          {mc.label}
                        </button>
                      );
                    })}
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          </div>

          {/* Textarea */}
          <textarea
            ref={textareaRef}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={handleKeyDown}
            onFocus={() => setIsFocused(true)}
            onBlur={() => setIsFocused(false)}
            placeholder={disabled ? (disabledReason || "Not ready") : config.placeholder}
            disabled={isRunning || disabled}
            rows={2}
            className={cn(
              "w-full bg-transparent text-text-0 text-[14px] leading-[1.7]",
              "placeholder:text-text-3/60",
              "resize-none outline-none",
              "px-5 pt-3 pb-14",
              "min-h-[88px] max-h-[200px]",
            )}
          />

          {/* Bottom toolbar */}
          <div className="absolute bottom-0 left-0 right-0 flex items-center justify-between px-4 pb-3.5">
            <span className="text-[11px] text-text-3/40">
              {!isRunning && !disabled && (
                <>
                  <kbd className="inline-flex items-center justify-center px-1.5 h-[18px] rounded bg-white/[0.06] text-text-3/50 text-[10px] font-mono leading-none">↵</kbd>
                  <span className="ml-1.5">to send</span>
                </>
              )}
            </span>

            <AnimatePresence mode="wait">
              {isRunning ? (
                <motion.button
                  key="cancel"
                  initial={{ scale: 0.8, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  exit={{ scale: 0.8, opacity: 0 }}
                  transition={{ duration: 0.15 }}
                  onClick={onCancel}
                  className="flex items-center justify-center w-9 h-9 rounded-xl bg-err text-white hover:bg-err/80 transition-colors shadow-sm shadow-err/20"
                  title="Stop agent"
                >
                  <Square size={14} />
                </motion.button>
              ) : (
                <motion.button
                  key="send"
                  initial={{ scale: 0.8, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  exit={{ scale: 0.8, opacity: 0 }}
                  transition={{ duration: 0.15 }}
                  whileTap={canSubmit ? { scale: 0.92 } : undefined}
                  onClick={handleSubmit}
                  disabled={!canSubmit}
                  className={cn(
                    "flex items-center justify-center w-9 h-9 rounded-xl transition-all duration-200",
                    canSubmit
                      ? "bg-brand text-white hover:bg-brand-hover shadow-lg shadow-brand/30"
                      : "bg-white/[0.05] text-text-3/40",
                  )}
                  title="Send message"
                >
                  {isRunning ? <Loader2 size={16} className="animate-spin" /> : <ArrowUp size={16} />}
                </motion.button>
              )}
            </AnimatePresence>
          </div>
        </motion.div>
      </div>
    </div>
  );
}
