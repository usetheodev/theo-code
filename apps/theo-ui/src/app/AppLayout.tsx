import { useEffect, useState } from "react";
import { Outlet } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { AppSidebar } from "./AppSidebar";
import { AppConfig, AuthStatus, AppLayoutContext } from "../types";

export function AppLayout() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [authStatus, setAuthStatus] = useState<AuthStatus | null>(null);
  const [projectDir, setProjectDir] = useState<string | null>(null);

  useEffect(() => {
    loadState();
  }, []);

  async function loadState() {
    try {
      const [cfg, auth, dir] = await Promise.all([
        invoke<AppConfig>("get_config"),
        invoke<AuthStatus>("auth_status"),
        invoke<string | null>("get_project_dir"),
      ]);
      setConfig(cfg);
      setAuthStatus(auth);
      setProjectDir(dir);

      if (auth.authenticated) {
        await invoke("auth_apply_to_config");
        setConfig(await invoke<AppConfig>("get_config"));
      }
    } catch (e) {
      console.error("Failed to load state:", e);
    }
  }

  const ctx: AppLayoutContext = {
    config,
    authStatus,
    projectDir,
    reloadState: loadState,
  };

  return (
    <SidebarProvider>
      <AppSidebar
        projectName={projectDir?.split("/").pop() || undefined}
        isConnected={!!config?.has_api_key}
      />
      <SidebarInset>
        <Outlet context={ctx} />
      </SidebarInset>
    </SidebarProvider>
  );
}
