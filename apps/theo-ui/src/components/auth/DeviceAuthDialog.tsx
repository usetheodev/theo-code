import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as Dialog from "@radix-ui/react-dialog";
import { motion, AnimatePresence } from "framer-motion";
import { Copy, Check, X, ExternalLink, Loader2, GitBranch } from "lucide-react";
import { useDeviceAuth, type DeviceAuthState } from "../../hooks/useDeviceAuth";

interface DeviceAuthDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSuccess: () => void;
}

export function DeviceAuthDialog({ open, onOpenChange, onSuccess }: DeviceAuthDialogProps) {
  const [state, actions] = useDeviceAuth();
  const [copied, setCopied] = useState(false);
  const [enterprise, setEnterprise] = useState(false);
  const [enterpriseUrl, setEnterpriseUrl] = useState("");

  // Start flow when dialog opens
  useEffect(() => {
    if (open && state.status === "idle") {
      actions.start(enterprise ? enterpriseUrl : undefined);
    }
  }, [open]);

  // Close on success
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

  // Cancel on close
  function handleOpenChange(value: boolean) {
    if (!value && state.status === "polling") {
      actions.cancel();
    }
    if (!value) {
      actions.reset();
    }
    onOpenChange(value);
  }

  async function handleCopy(code: string) {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch { /* noop */ }
  }

  async function handleOpenBrowser(url: string) {
    try {
      // Use Tauri shell plugin if available, fallback to window.open
      await invoke("plugin:shell|open", { path: url }).catch(() => {
        window.open(url, "_blank");
      });
    } catch { /* noop */ }
  }

  function handleRetry() {
    actions.reset();
    setTimeout(() => {
      actions.start(enterprise ? enterpriseUrl : undefined);
    }, 100);
  }

  return (
    <Dialog.Root open={open} onOpenChange={handleOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/50 z-50" />
        <Dialog.Content
          className="fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[420px] bg-surface-1 border border-border rounded-xl shadow-xl p-6 outline-none"
          onInteractOutside={(e) => {
            if (state.status === "polling") e.preventDefault();
          }}
        >
          <div className="flex items-center justify-between mb-4">
            <Dialog.Title className="text-[15px] font-semibold text-text-0 flex items-center gap-2">
              <GitBranch size={18} />
              GitHub Copilot
            </Dialog.Title>
            <Dialog.Close className="text-text-3 hover:text-text-1 transition-colors">
              <X size={16} />
            </Dialog.Close>
          </div>

          <AnimatePresence mode="wait">
            {state.status === "requesting" && <RequestingView key="requesting" />}
            {state.status === "polling" && (
              <PollingView
                key="polling"
                state={state}
                copied={copied}
                onCopy={handleCopy}
                onOpenBrowser={handleOpenBrowser}
              />
            )}
            {state.status === "success" && <SuccessView key="success" />}
            {state.status === "error" && (
              <ErrorView key="error" message={state.message} onRetry={handleRetry} />
            )}
            {state.status === "idle" && (
              <IdleView
                key="idle"
                enterprise={enterprise}
                enterpriseUrl={enterpriseUrl}
                onEnterpriseToggle={setEnterprise}
                onEnterpriseUrlChange={setEnterpriseUrl}
                onStart={() => actions.start(enterprise ? enterpriseUrl : undefined)}
              />
            )}
          </AnimatePresence>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function RequestingView() {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="flex flex-col items-center py-8"
    >
      <Loader2 size={24} className="animate-spin text-text-2" />
      <p className="text-[13px] text-text-2 mt-3">Connecting to GitHub...</p>
    </motion.div>
  );
}

function PollingView({
  state,
  copied,
  onCopy,
  onOpenBrowser,
}: {
  state: Extract<DeviceAuthState, { status: "polling" }>;
  copied: boolean;
  onCopy: (code: string) => void;
  onOpenBrowser: (url: string) => void;
}) {
  const [secondsLeft, setSecondsLeft] = useState(
    Math.max(0, Math.floor((state.expiresAt - Date.now()) / 1000))
  );

  useEffect(() => {
    const interval = setInterval(() => {
      setSecondsLeft(Math.max(0, Math.floor((state.expiresAt - Date.now()) / 1000)));
    }, 1000);
    return () => clearInterval(interval);
  }, [state.expiresAt]);

  const minutes = Math.floor(secondsLeft / 60);
  const seconds = secondsLeft % 60;
  const progress = secondsLeft / 900;

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -8 }}
      className="flex flex-col gap-4"
    >
      {/* Device code */}
      <div className="bg-surface-0 border border-border rounded-lg p-4 text-center">
        <p className="text-[11px] text-text-3 mb-2">Enter this code on GitHub</p>
        <button
          onClick={() => onCopy(state.userCode)}
          className="group inline-flex items-center gap-2"
          aria-label={`Verification code: ${state.userCode.split("").join(" ")}`}
        >
          <span className="text-[28px] font-mono font-bold text-text-0 tracking-[0.2em]">
            {state.userCode}
          </span>
          <AnimatePresence mode="wait">
            {copied ? (
              <motion.span
                key="check"
                initial={{ scale: 0.5, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                exit={{ scale: 0.5, opacity: 0 }}
              >
                <Check size={16} className="text-ok" />
              </motion.span>
            ) : (
              <motion.span
                key="copy"
                initial={{ scale: 0.5, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                exit={{ scale: 0.5, opacity: 0 }}
              >
                <Copy size={16} className="text-text-3 group-hover:text-text-1 transition-colors" />
              </motion.span>
            )}
          </AnimatePresence>
        </button>
        <p className="text-[11px] text-text-3 mt-1">
          {copied ? "Copied to clipboard" : "Click to copy"}
        </p>
      </div>

      {/* Open browser button */}
      <button
        onClick={() => onOpenBrowser(state.verificationUri)}
        className="flex items-center justify-center gap-2 px-4 py-2.5 bg-surface-3 border border-border rounded-lg text-[13px] text-text-0 hover:bg-surface-4 transition-colors"
      >
        <ExternalLink size={14} />
        Open {state.verificationUri}
      </button>

      {/* Polling indicator */}
      <div className="flex flex-col items-center gap-2 pt-2">
        <div className="flex gap-1">
          {[0, 1, 2].map((i) => (
            <motion.div
              key={i}
              className="w-1.5 h-1.5 rounded-full bg-text-3"
              animate={{ y: [0, -4, 0] }}
              transition={{
                duration: 0.6,
                repeat: Infinity,
                delay: i * 0.15,
                ease: "easeInOut",
              }}
            />
          ))}
        </div>
        <p className="text-[12px] text-text-3">Waiting for authorization...</p>
      </div>

      {/* Countdown timer */}
      <div className="flex flex-col items-center gap-1">
        <div className="w-full h-0.5 bg-surface-0 rounded-full overflow-hidden">
          <motion.div
            className={`h-full rounded-full ${secondsLeft < 60 ? "bg-warn" : "bg-brand"}`}
            style={{ width: `${progress * 100}%` }}
            transition={{ duration: 0.5 }}
          />
        </div>
        <p className={`text-[11px] ${secondsLeft < 60 ? "text-warn" : "text-text-3"}`} role="timer">
          Code expires in {minutes}:{seconds.toString().padStart(2, "0")}
        </p>
      </div>
    </motion.div>
  );
}

function SuccessView() {
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0 }}
      className="flex flex-col items-center py-8"
    >
      <motion.div
        initial={{ scale: 0 }}
        animate={{ scale: 1 }}
        transition={{ type: "spring", stiffness: 300, damping: 20 }}
        className="w-12 h-12 rounded-full bg-ok/10 flex items-center justify-center mb-3"
      >
        <Check size={24} className="text-ok" />
      </motion.div>
      <p className="text-[14px] font-medium text-text-0">Connected to GitHub Copilot</p>
      <p className="text-[12px] text-text-3 mt-1">Closing automatically...</p>
    </motion.div>
  );
}

function ErrorView({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="flex flex-col items-center py-6"
    >
      <div className="w-12 h-12 rounded-full bg-err/10 flex items-center justify-center mb-3">
        <X size={24} className="text-err" />
      </div>
      <p className="text-[14px] font-medium text-text-0 mb-1">Authorization failed</p>
      <p className="text-[12px] text-text-3 text-center mb-4 max-w-[300px]">{message}</p>
      <button
        onClick={onRetry}
        className="px-4 py-2 bg-surface-3 border border-border rounded-lg text-[13px] text-text-0 hover:bg-surface-4 transition-colors"
      >
        Try again
      </button>
    </motion.div>
  );
}

function IdleView({
  enterprise,
  enterpriseUrl,
  onEnterpriseToggle,
  onEnterpriseUrlChange,
  onStart,
}: {
  enterprise: boolean;
  enterpriseUrl: string;
  onEnterpriseToggle: (v: boolean) => void;
  onEnterpriseUrlChange: (v: string) => void;
  onStart: () => void;
}) {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="flex flex-col gap-4"
    >
      <p className="text-[13px] text-text-2">
        Sign in with your GitHub account to use Copilot models.
      </p>

      <button
        onClick={onStart}
        className="flex items-center justify-center gap-2 px-4 py-2.5 bg-brand text-white rounded-lg text-[13px] font-medium hover:bg-brand-hover transition-colors"
      >
        <GitBranch size={16} />
        Login with GitHub
      </button>

      {/* Enterprise toggle */}
      <div>
        <button
          onClick={() => onEnterpriseToggle(!enterprise)}
          className="text-[12px] text-text-3 hover:text-text-1 transition-colors flex items-center gap-1"
        >
          <motion.span
            animate={{ rotate: enterprise ? 90 : 0 }}
            className="inline-block"
          >
            &#9656;
          </motion.span>
          GitHub Enterprise
        </button>
        <AnimatePresence>
          {enterprise && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              className="overflow-hidden"
            >
              <input
                value={enterpriseUrl}
                onChange={(e) => onEnterpriseUrlChange(e.target.value)}
                placeholder="https://github.company.com"
                className="w-full mt-2 px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-0 placeholder:text-text-3 outline-none focus:border-border-focus transition-colors"
              />
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </motion.div>
  );
}
