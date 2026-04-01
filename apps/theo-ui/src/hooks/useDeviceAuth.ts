import { useState, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

export type DeviceAuthState =
  | { status: "idle" }
  | { status: "requesting" }
  | { status: "polling"; userCode: string; verificationUri: string; expiresAt: number }
  | { status: "success"; domain?: string }
  | { status: "error"; message: string };

export interface DeviceAuthActions {
  start: (enterpriseUrl?: string) => Promise<void>;
  cancel: () => void;
  reset: () => void;
}

interface DeviceCodeResponse {
  user_code: string;
  verification_uri: string;
  device_code: string;
  interval: number;
}

interface PollResponse {
  success: boolean;
  domain?: string;
}

export function useDeviceAuth(): [DeviceAuthState, DeviceAuthActions] {
  const [state, setState] = useState<DeviceAuthState>({ status: "idle" });
  const cancelledRef = useRef(false);
  const pollAbortRef = useRef<AbortController | null>(null);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      cancelledRef.current = true;
      pollAbortRef.current?.abort();
    };
  }, []);

  const start = useCallback(async (enterpriseUrl?: string) => {
    cancelledRef.current = false;
    setState({ status: "requesting" });

    try {
      // Step 1: Start device flow
      const dc = await invoke<DeviceCodeResponse>("copilot_start_device_flow", {
        enterpriseUrl: enterpriseUrl || null,
      });

      if (cancelledRef.current) return;

      // Step 2: Show code to user
      const expiresAt = Date.now() + 15 * 60 * 1000; // 15 minutes
      setState({
        status: "polling",
        userCode: dc.user_code,
        verificationUri: dc.verification_uri,
        expiresAt,
      });

      // Copy code to clipboard
      try {
        await navigator.clipboard.writeText(dc.user_code);
      } catch {
        // Clipboard may not be available — not critical
      }

      // Step 3: Poll in background
      const result = await invoke<PollResponse>("copilot_poll_device_flow", {
        deviceCode: dc.device_code,
        interval: dc.interval,
        enterpriseUrl: enterpriseUrl || null,
      });

      if (cancelledRef.current) return;

      // Step 4: Apply config
      await invoke<boolean>("copilot_apply_to_config");

      setState({ status: "success", domain: result.domain });
    } catch (e) {
      if (cancelledRef.current) return;
      setState({
        status: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  const cancel = useCallback(() => {
    cancelledRef.current = true;
    pollAbortRef.current?.abort();
    setState({ status: "idle" });
  }, []);

  const reset = useCallback(() => {
    cancelledRef.current = false;
    setState({ status: "idle" });
  }, []);

  return [state, { start, cancel, reset }];
}
