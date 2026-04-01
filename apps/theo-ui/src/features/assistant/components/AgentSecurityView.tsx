import { ShieldCheck } from "lucide-react";

export function AgentSecurityView() {
  return (
    <div className="flex-1 flex items-center justify-center">
      <div className="flex flex-col items-center gap-4 text-center animate-fade-in">
        <div className="w-12 h-12 rounded-2xl bg-surface-2 ring-1 ring-white/[0.06] flex items-center justify-center">
          <ShieldCheck size={22} className="text-text-3" />
        </div>
        <div>
          <h3 className="text-[14px] font-medium text-text-1 mb-1">Agent Security</h3>
          <p className="text-[13px] text-text-3 max-w-xs">
            Security audit results, vulnerability scanning,
            and safety analysis from the agent will appear here.
          </p>
        </div>
      </div>
    </div>
  );
}
