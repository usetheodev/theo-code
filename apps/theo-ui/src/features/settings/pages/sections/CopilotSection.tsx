import { LogOut, GitBranch } from "lucide-react";
import { DeviceAuthDialog } from "../../../../components/auth/DeviceAuthDialog";
import { CopilotStatus } from "../../hooks/useSettings";
import { Section } from "./Section";

export interface CopilotSectionProps {
  status: CopilotStatus | null;
  dialogOpen: boolean;
  onDialogOpenChange: (open: boolean) => void;
  onLogout: () => Promise<void>;
  onSuccess: () => Promise<void>;
}

export function CopilotSection({
  status,
  dialogOpen,
  onDialogOpenChange,
  onLogout,
  onSuccess,
}: CopilotSectionProps) {
  return (
    <Section title="GitHub Copilot">
      {status?.authenticated ? (
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-ok" />
            <span className="text-[13px] text-text-1">
              Connected
              {status.domain && status.domain !== "github.com" && (
                <span className="text-text-3 ml-1">({status.domain})</span>
              )}
            </span>
          </div>
          <button
            onClick={onLogout}
            className="flex items-center gap-1.5 px-3 py-1.5 text-[12px] text-text-2 hover:text-err border border-border rounded-md hover:border-err/30 transition-colors"
          >
            <LogOut size={12} />
            Disconnect
          </button>
        </div>
      ) : (
        <div>
          <button
            onClick={() => onDialogOpenChange(true)}
            className="flex items-center gap-2 px-4 py-2 bg-surface-3 border border-border rounded-lg text-[13px] text-text-0 hover:bg-surface-4 transition-colors"
          >
            <GitBranch size={14} />
            Login with GitHub Copilot
          </button>
          <p className="text-[11px] text-text-3 mt-2">
            Use your GitHub Copilot subscription
          </p>
        </div>
      )}
      <DeviceAuthDialog
        open={dialogOpen}
        onOpenChange={onDialogOpenChange}
        onSuccess={onSuccess}
      />
    </Section>
  );
}
