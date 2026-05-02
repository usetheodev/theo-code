import {
  MessageSquare,
  Rocket,
  Activity,
  Database,
  Settings,
  CodeIcon,
  Zap,
  ChevronsLeft,
  User,
  ScrollText,
  Bot,
} from "lucide-react";
import { useLocation, useNavigate } from "react-router-dom";

import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarSeparator,
  useSidebar,
} from "@/components/ui/sidebar";

const MAIN_ITEMS = [
  { id: "assistant", label: "Assistant", icon: MessageSquare },
];

const WORKSPACE_ITEMS = [
  { id: "observability", label: "Observability", icon: ScrollText },
  { id: "agents", label: "Agents", icon: Bot },
  { id: "deploys", label: "Deploys", icon: Rocket },
  { id: "monitoring", label: "Monitoring", icon: Activity },
  { id: "database", label: "Database", icon: Database },
  { id: "code", label: "Code", icon: CodeIcon },
];

interface AppSidebarProps {
  projectName?: string;
  isConnected: boolean;
}

export function AppSidebar({ projectName, isConnected }: AppSidebarProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const activeItem = location.pathname.split("/")[1] || "assistant";

  return (
    <Sidebar collapsible="icon">
      {/* Header: logo + collapse */}
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg" className="pointer-events-none">
              <div className="flex aspect-square size-8 items-center justify-center rounded-lg bg-gradient-to-br from-brand to-brand-hover text-white shadow-md shadow-brand/20">
                <Zap className="size-4" />
              </div>
              <div className="flex flex-col gap-0.5 leading-none">
                <span className="font-semibold text-sm">Theo</span>
                {projectName && (
                  <span className="text-xs text-sidebar-foreground/50 truncate">
                    {projectName}
                  </span>
                )}
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      {/* Navigation */}
      <SidebarContent>
        {/* Primary */}
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              {MAIN_ITEMS.map((item) => (
                <SidebarMenuItem key={item.id}>
                  <SidebarMenuButton
                    isActive={activeItem === item.id}
                    onClick={() => navigate(`/${item.id}`)}
                    tooltip={item.label}
                  >
                    <item.icon />
                    <span>{item.label}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        {/* Workspace */}
        <SidebarGroup>
          <SidebarGroupLabel>Workspace</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {WORKSPACE_ITEMS.map((item) => (
                <SidebarMenuItem key={item.id}>
                  <SidebarMenuButton
                    isActive={activeItem === item.id}
                    onClick={() => navigate(`/${item.id}`)}
                    tooltip={item.label}
                  >
                    <item.icon />
                    <span>{item.label}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      {/* Footer: account + settings + collapse toggle */}
      <SidebarFooter>
        <SidebarSeparator />

        {/* Account */}
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton tooltip="Account" className="pointer-events-none h-10">
              <div className="flex aspect-square size-7 items-center justify-center rounded-full bg-gradient-to-br from-brand/30 to-brand/10 ring-1 ring-white/[0.08]">
                <User className="size-3.5 text-brand" />
              </div>
              <div className="flex flex-col gap-0 leading-tight">
                <span className="text-[13px] font-medium truncate">Paulo</span>
                <span className="text-[10px] text-sidebar-foreground/50 truncate flex items-center gap-1.5">
                  <span className={`size-1.5 rounded-full shrink-0 ${isConnected ? "bg-ok shadow-[0_0_4px_rgba(16,185,129,0.5)]" : "bg-err shadow-[0_0_4px_rgba(239,68,68,0.4)]"}`} />
                  {isConnected ? "Connected" : "Not connected"}
                </span>
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>

        {/* Settings */}
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              isActive={activeItem === "settings"}
              onClick={() => navigate("/settings")}
              tooltip="Settings"
            >
              <Settings />
              <span>Settings</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>

        {/* Collapse toggle */}
        <CollapseButton />
      </SidebarFooter>
    </Sidebar>
  );
}

function CollapseButton() {
  const { toggleSidebar, state } = useSidebar();

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <SidebarMenuButton
          onClick={toggleSidebar}
          tooltip={state === "expanded" ? "Collapse" : "Expand"}
          className="text-sidebar-foreground/40 hover:text-sidebar-foreground/80"
        >
          <ChevronsLeft
            className={`transition-transform duration-200 ${state === "collapsed" ? "rotate-180" : ""}`}
          />
          <span>Collapse</span>
        </SidebarMenuButton>
      </SidebarMenuItem>
    </SidebarMenu>
  );
}
