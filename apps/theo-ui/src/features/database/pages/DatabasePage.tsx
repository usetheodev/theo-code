import { Database } from "lucide-react";

export function DatabasePage() {
  return (
    <div className="flex-1 flex flex-col">
      <div className="flex items-center justify-between px-6 h-[52px] border-b border-border shrink-0">
        <h2 className="text-[15px] font-semibold text-text-0">Database</h2>
      </div>
      <div className="flex-1 flex items-center justify-center">
        <div className="flex flex-col items-center gap-4 text-center animate-fade-in">
          <div className="w-12 h-12 rounded-2xl bg-surface-3 flex items-center justify-center">
            <Database size={22} className="text-text-3" />
          </div>
          <div>
            <h3 className="text-[14px] font-medium text-text-1 mb-1">No database connected</h3>
            <p className="text-[13px] text-text-3 max-w-xs">
              Connect a database to browse tables, run queries,
              and manage data directly from Theo.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
