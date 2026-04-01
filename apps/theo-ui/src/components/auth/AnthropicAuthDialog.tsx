import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as Dialog from "@radix-ui/react-dialog";
import { motion, AnimatePresence } from "framer-motion";
import { Copy, Check, X, ExternalLink, Loader2 } from "lucide-react";
import { useAnthropicAuth, type AnthropicAuthState } from "../../hooks/useAnthropicAuth";

interface AnthropicAuthDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSuccess: () => void;
}

export function AnthropicAuthDialog({ open, onOpenChange, onSuccess }: AnthropicAuthDialogProps) {
  const [state, actions] = useAnthropicAuth();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (open && state.status === "idle") {
      actions.start();
    }
  }, [open]);

  useEffect(() => {
    if (state.status === "success") {
      const timer = setTimeout(() => {
        onSuccess();
        onOpenChange(false);
        actions.reset();
      }, 1500);
      return () => clearTimeout(timer);
    }
  }, [state.status]);

  function handleOpenChange(value: boolean) {
    if (!value && state.status === "polling") actions.cancel();
    if (!value) actions.reset();
    onOpenChange(value);
  }

  async function handleCopy(code: string) {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {}
  }

  async function handleOpenBrowser(url: string) {
    try {
      await invoke("plugin:shell|open", { path: url }).catch(() => {
        window.open(url, "_blank");
      });
    } catch {}
  }

  return (
    <Dialog.Root open={open} onOpenChange={handleOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/50 z-50" />
        <Dialog.Content
          className="fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[420px] bg-surface-1 border border-border rounded-xl shadow-xl p-6 outline-none"
          onInteractOutside={(e) => { if (state.status === "polling") e.preventDefault(); }}
        >
          <div className="flex items-center justify-between mb-4">
            <Dialog.Title className="text-[15px] font-semibold text-text-0">
              Anthropic Console
            </Dialog.Title>
            <Dialog.Close className="text-text-3 hover:text-text-1 transition-colors">
              <X size={16} />
            </Dialog.Close>
          </div>

          <AnimatePresence mode="wait">
            {state.status === "requesting" && (
              <motion.div key="req" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="flex flex-col items-center py-8">
                <Loader2 size={24} className="animate-spin text-text-2" />
                <p className="text-[13px] text-text-2 mt-3">Connecting to Anthropic...</p>
              </motion.div>
            )}

            {state.status === "polling" && (
              <motion.div key="poll" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} className="flex flex-col gap-4">
                <div className="bg-surface-0 border border-border rounded-lg p-4 text-center">
                  <p className="text-[11px] text-text-3 mb-2">Enter this code on Anthropic</p>
                  <button onClick={() => handleCopy(state.userCode)} className="group inline-flex items-center gap-2">
                    <span className="text-[28px] font-mono font-bold text-text-0 tracking-[0.2em]">{state.userCode}</span>
                    {copied ? <Check size={16} className="text-ok" /> : <Copy size={16} className="text-text-3 group-hover:text-text-1" />}
                  </button>
                  <p className="text-[11px] text-text-3 mt-1">{copied ? "Copied" : "Click to copy"}</p>
                </div>
                <button
                  onClick={() => handleOpenBrowser(state.verificationUri)}
                  className="flex items-center justify-center gap-2 px-4 py-2.5 bg-surface-3 border border-border rounded-lg text-[13px] text-text-0 hover:bg-surface-4 transition-colors"
                >
                  <ExternalLink size={14} />
                  Open Anthropic Console
                </button>
                <div className="flex flex-col items-center gap-2 pt-2">
                  <div className="flex gap-1">
                    {[0, 1, 2].map((i) => (
                      <motion.div key={i} className="w-1.5 h-1.5 rounded-full bg-text-3"
                        animate={{ y: [0, -4, 0] }}
                        transition={{ duration: 0.6, repeat: Infinity, delay: i * 0.15, ease: "easeInOut" }}
                      />
                    ))}
                  </div>
                  <p className="text-[12px] text-text-3">Waiting for authorization...</p>
                </div>
              </motion.div>
            )}

            {state.status === "success" && (
              <motion.div key="ok" initial={{ opacity: 0, scale: 0.95 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0 }} className="flex flex-col items-center py-8">
                <motion.div initial={{ scale: 0 }} animate={{ scale: 1 }} transition={{ type: "spring", stiffness: 300, damping: 20 }}
                  className="w-12 h-12 rounded-full bg-ok/10 flex items-center justify-center mb-3">
                  <Check size={24} className="text-ok" />
                </motion.div>
                <p className="text-[14px] font-medium text-text-0">Connected to Anthropic</p>
                {state.email && <p className="text-[12px] text-text-3 mt-1">{state.email}</p>}
              </motion.div>
            )}

            {state.status === "error" && (
              <motion.div key="err" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="flex flex-col items-center py-6">
                <div className="w-12 h-12 rounded-full bg-err/10 flex items-center justify-center mb-3">
                  <X size={24} className="text-err" />
                </div>
                <p className="text-[14px] font-medium text-text-0 mb-1">Authorization failed</p>
                <p className="text-[12px] text-text-3 text-center mb-4 max-w-[300px]">{state.message}</p>
                <button onClick={() => { actions.reset(); setTimeout(() => actions.start(), 100); }}
                  className="px-4 py-2 bg-surface-3 border border-border rounded-lg text-[13px] text-text-0 hover:bg-surface-4 transition-colors">
                  Try again
                </button>
              </motion.div>
            )}
          </AnimatePresence>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
