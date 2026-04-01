import { TestTube2 } from "lucide-react";

export function AgentTestsView() {
  return (
    <div className="flex-1 flex items-center justify-center">
      <div className="flex flex-col items-center gap-4 text-center animate-fade-in">
        <div className="w-12 h-12 rounded-2xl bg-surface-2 ring-1 ring-white/[0.06] flex items-center justify-center">
          <TestTube2 size={22} className="text-text-3" />
        </div>
        <div>
          <h3 className="text-[14px] font-medium text-text-1 mb-1">Agent Tests</h3>
          <p className="text-[13px] text-text-3 max-w-xs">
            Automated test generation and execution results will appear here.
            The agent can write and run tests for your codebase.
          </p>
        </div>
      </div>
    </div>
  );
}
