import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { cn } from "@/lib/utils";
import { Copy, Check, ChevronDown, Brain } from "lucide-react";

interface AssistantMessageProps {
  content: string;
  reasoning?: string;
  isStreaming?: boolean;
  isDone?: boolean;
}

export function AssistantMessage({ content, reasoning, isStreaming, isDone }: AssistantMessageProps) {
  const [copied, setCopied] = useState(false);
  const [showReasoning, setShowReasoning] = useState(false);
  const [hovered, setHovered] = useState(false);

  if (!content && !reasoning) return null;

  const handleCopy = async () => {
    await navigator.clipboard.writeText(content);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const hasReasoning = !!reasoning && reasoning.length > 0;

  return (
    <div
      className="max-w-[85%] group/msg relative"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {/* Reasoning collapsible */}
      {hasReasoning && (
        <div className="mb-2">
          <button
            onClick={() => setShowReasoning(!showReasoning)}
            className="flex items-center gap-1.5 px-2 py-1 rounded-md text-[11px] text-text-3 hover:text-text-2 hover:bg-white/[0.03] transition-all"
          >
            <Brain size={12} className="text-brand/50" />
            <span>{showReasoning ? "Hide analysis" : "Show analysis"}</span>
            <motion.div
              animate={{ rotate: showReasoning ? 180 : 0 }}
              transition={{ duration: 0.15 }}
            >
              <ChevronDown size={11} />
            </motion.div>
          </button>

          <AnimatePresence>
            {showReasoning && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
                className="overflow-hidden"
              >
                <div className="mt-1.5 px-3 py-2.5 rounded-lg bg-surface-1 border border-white/[0.04] text-[12px] text-text-2 leading-relaxed">
                  {reasoning}
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      )}

      {/* Main content */}
      <div className="text-[14px] leading-[1.7] text-text-0">
        <RichContent text={content} />
        {isStreaming && (
          <span className="inline-block w-[2px] h-[1em] bg-brand/70 ml-0.5 align-text-bottom animate-pulse" />
        )}
      </div>

      {/* Actions toolbar — appears on hover after streaming is done */}
      <AnimatePresence>
        {hovered && !isStreaming && content && (
          <motion.div
            initial={{ opacity: 0, y: 2 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 2 }}
            transition={{ duration: 0.12 }}
            className="flex items-center gap-0.5 mt-1.5"
          >
            <ActionButton
              onClick={handleCopy}
              icon={copied ? Check : Copy}
              label={copied ? "Copied" : "Copy"}
              active={copied}
            />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function ActionButton({
  onClick,
  icon: Icon,
  label,
  active,
}: {
  onClick: () => void;
  icon: React.ComponentType<any>;
  label: string;
  active?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex items-center gap-1 px-1.5 py-1 rounded-md text-[11px] transition-all",
        active
          ? "text-ok"
          : "text-text-3 hover:text-text-1 hover:bg-white/[0.04]",
      )}
      title={label}
    >
      <Icon size={12} />
      <span>{label}</span>
    </button>
  );
}

function RichContent({ text }: { text: string }) {
  const parts = text.split(/(```[\s\S]*?```)/g);

  return (
    <>
      {parts.map((part, i) => {
        if (part.startsWith("```") && part.endsWith("```")) {
          const lines = part.slice(3, -3);
          const firstNewline = lines.indexOf("\n");
          const code = firstNewline >= 0 ? lines.slice(firstNewline + 1) : lines;
          const lang = firstNewline >= 0 ? lines.slice(0, firstNewline).trim() : "";

          return <CodeBlock key={i} code={code} lang={lang} />;
        }

        const inlineParts = part.split(/(`[^`]+`)/g);
        return (
          <span key={i}>
            {inlineParts.map((ip, j) => {
              if (ip.startsWith("`") && ip.endsWith("`")) {
                return (
                  <code
                    key={j}
                    className="px-1.5 py-0.5 rounded bg-surface-3 font-mono text-[13px] text-brand"
                  >
                    {ip.slice(1, -1)}
                  </code>
                );
              }
              return <span key={j}>{ip}</span>;
            })}
          </span>
        );
      })}
    </>
  );
}

function CodeBlock({ code, lang }: { code: string; lang: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="my-3 rounded-lg border border-white/[0.06] overflow-hidden group/code">
      <div className="flex items-center justify-between px-3 py-1.5 bg-surface-3 border-b border-white/[0.06]">
        <span className="text-[11px] text-text-3 font-mono">{lang || "code"}</span>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1 text-[10px] text-text-3 hover:text-text-1 transition-colors opacity-0 group-hover/code:opacity-100"
        >
          {copied ? <Check size={11} className="text-ok" /> : <Copy size={11} />}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="p-3 bg-surface-2 overflow-x-auto">
        <code className="text-[13px] font-mono text-text-1 leading-relaxed">{code}</code>
      </pre>
    </div>
  );
}
