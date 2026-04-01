import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { AgentEventType, ChatMessage } from "../types";

let messageId = 0;
const nextId = () => `msg_${++messageId}`;

// Patterns that indicate reasoning/thinking content
const REASONING_PATTERNS = [
  /^<think>/i,
  /^<reasoning>/i,
  /^\[thinking\]/i,
  /^Let me (think|analyze|consider|reason)/i,
  /^I need to (think|analyze|consider|understand)/i,
];

function splitReasoningFromContent(raw: string): { reasoning: string; content: string } {
  // Handle <think>...</think> blocks
  const thinkMatch = raw.match(/^<think>([\s\S]*?)<\/think>\s*([\s\S]*)$/i);
  if (thinkMatch) {
    return { reasoning: thinkMatch[1].trim(), content: thinkMatch[2].trim() };
  }

  // Handle <reasoning>...</reasoning> blocks
  const reasonMatch = raw.match(/^<reasoning>([\s\S]*?)<\/reasoning>\s*([\s\S]*)$/i);
  if (reasonMatch) {
    return { reasoning: reasonMatch[1].trim(), content: reasonMatch[2].trim() };
  }

  // No reasoning detected
  return { reasoning: "", content: raw };
}

export function useAgentEvents(
  setMessages: React.Dispatch<React.SetStateAction<ChatMessage[]>>,
  setIsRunning: React.Dispatch<React.SetStateAction<boolean>>,
) {
  const assistantBuffer = useRef("");

  useEffect(() => {
    const unlisten = listen<AgentEventType>("agent-event", (event) => {
      const e = event.payload;

      switch (e.type) {
        case "token": {
          assistantBuffer.current += e.text;
          const raw = assistantBuffer.current;
          const { reasoning, content } = splitReasoningFromContent(raw);

          setMessages((prev) => {
            const last = prev[prev.length - 1];
            if (last && last.role === "assistant" && !last.toolName && !last.done) {
              return [
                ...prev.slice(0, -1),
                { ...last, content: content || raw, reasoning: reasoning || last.reasoning },
              ];
            }
            return [
              ...prev,
              { id: nextId(), role: "assistant", content: content || raw, reasoning },
            ];
          });
          break;
        }

        case "tool_start": {
          assistantBuffer.current = "";
          setMessages((prev) => [
            ...prev,
            {
              id: nextId(),
              role: "tool",
              content: "",
              toolName: e.name,
              toolArgs: e.args,
              collapsed: true,
            },
          ]);
          break;
        }

        case "tool_end": {
          setMessages((prev) => {
            const idx = [...prev].reverse().findIndex(
              (m) => m.role === "tool" && m.toolName === e.name && m.content === ""
            );
            if (idx === -1) return prev;
            const realIdx = prev.length - 1 - idx;
            const updated = [...prev];
            updated[realIdx] = {
              ...updated[realIdx],
              content: e.output,
              toolSuccess: e.success,
            };
            return updated;
          });
          break;
        }

        case "done": {
          assistantBuffer.current = "";
          setIsRunning(false);

          setMessages((prev) => {
            // Mark the last assistant message as done
            const updated = [...prev];
            for (let i = updated.length - 1; i >= 0; i--) {
              if (updated[i].role === "assistant" && !updated[i].done) {
                updated[i] = { ...updated[i], done: true };
                break;
              }
            }
            // Summary from done() tool is internal metadata, not user-facing content.
            // Do NOT render it as a chat message.
            return updated;
          });
          break;
        }

        case "error": {
          setMessages((prev) => [
            ...prev,
            { id: nextId(), role: "assistant", content: `Error: ${e.message}`, done: true },
          ]);
          break;
        }

        case "llm_call_start":
          setIsRunning(true);
          assistantBuffer.current = "";
          break;

        case "llm_call_end":
        case "phase_change":
          break;
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [setMessages, setIsRunning]);
}
