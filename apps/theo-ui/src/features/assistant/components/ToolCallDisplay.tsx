import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { cn } from "@/lib/utils";
import { ChevronRight, Check, X, Loader2 } from "lucide-react";
import { ChatMessage } from "../../../types";

export function ToolCallDisplay({ msg }: { msg: ChatMessage }) {
  const [expanded, setExpanded] = useState(false);

  const isRunning = msg.content === "";
  const isSuccess = !isRunning && msg.toolSuccess;
  const isError = !isRunning && !msg.toolSuccess;

  return (
    <div className="rounded-lg border border-white/[0.06] overflow-hidden bg-surface-1">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center gap-2 px-3 py-2 hover:bg-white/[0.03] transition-colors text-left"
      >
        <motion.div
          animate={{ rotate: expanded ? 90 : 0 }}
          transition={{ duration: 0.15 }}
        >
          <ChevronRight size={14} className="text-text-3" />
        </motion.div>
        <span className="text-[12px] font-mono font-medium text-brand">
          {msg.toolName}
        </span>
        <span className="ml-auto flex items-center gap-1">
          {isRunning && <Loader2 size={12} className="text-warn animate-spin" />}
          {isSuccess && (
            <motion.div initial={{ scale: 0 }} animate={{ scale: 1 }} transition={{ type: "spring", stiffness: 500, damping: 25 }}>
              <Check size={12} className="text-ok" />
            </motion.div>
          )}
          {isError && <X size={12} className="text-err" />}
        </span>
      </button>

      <AnimatePresence>
        {expanded && msg.content && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
            className="overflow-hidden"
          >
            <div className="px-3 py-2 border-t border-white/[0.06] bg-surface-0 font-mono text-[11px] text-text-2 max-h-[200px] overflow-y-auto whitespace-pre-wrap">
              {msg.content}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
