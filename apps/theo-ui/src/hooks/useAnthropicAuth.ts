import { useState, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

export type AnthropicAuthState =
  | { status: "idle" }
  | { status: "requesting" }
  | { status: "polling"; userCode: string; verificationUri: string; expiresAt: number }
  | { status: "success"; email?: string }
  | { status: "error"; message: string };

export function useAnthropicAuth(): [AnthropicAuthState, { start: () => Promise<void>; cancel: () => void; reset: () => void }] {
  const [state, setState] = useState<AnthropicAuthState>({ status: "idle" });
  const cancelledRef = useRef(false);

  useEffect(() => {
    return () => { cancelledRef.current = true; };
  }, []);

  const start = useCallback(async () => {
    cancelledRef.current = false;
    setState({ status: "requesting" });

    try {
      const dc = await invoke<{
        user_code: string;
        verification_uri: string;
        device_code: string;
        interval: number;
        expires_in: number;
      }>("anthropic_start_device_flow", { server: null });

      if (cancelledRef.current) return;

      setState({
        status: "polling",
        userCode: dc.user_code,
        verificationUri: dc.verification_uri,
        expiresAt: Date.now() + dc.expires_in * 1000,
      });

      try { await navigator.clipboard.writeText(dc.user_code); } catch {}

      const result = await invoke<{ success: boolean; email?: string }>(
        "anthropic_poll_device_flow",
        {
          deviceCode: dc.device_code,
          interval: dc.interval,
          expiresIn: dc.expires_in,
          server: null,
        },
      );

      if (cancelledRef.current) return;

      await invoke<boolean>("anthropic_apply_to_config", { model: null });

      setState({ status: "success", email: result.email });
    } catch (e) {
      if (cancelledRef.current) return;
      setState({ status: "error", message: e instanceof Error ? e.message : String(e) });
    }
  }, []);

  const cancel = useCallback(() => {
    cancelledRef.current = true;
    setState({ status: "idle" });
  }, []);

  const reset = useCallback(() => {
    cancelledRef.current = false;
    setState({ status: "idle" });
  }, []);

  return [state, { start, cancel, reset }];
}
