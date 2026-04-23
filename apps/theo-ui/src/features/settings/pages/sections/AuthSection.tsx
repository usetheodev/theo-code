import { LogOut, ExternalLink } from "lucide-react";
import { AuthStatus } from "../../../../types";
import { Section } from "./Section";

export interface AuthSectionProps {
  auth: AuthStatus | null;
  authLoading: boolean;
  onLogin: () => Promise<void>;
  onLogout: () => Promise<void>;
}

export function AuthSection({
  auth,
  authLoading,
  onLogin,
  onLogout,
}: AuthSectionProps) {
  return (
    <Section title="Authentication">
      {auth?.authenticated ? (
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-ok" />
            <span className="text-[13px] text-text-1">
              Connected
              {auth.account_id && (
                <span className="text-text-3 ml-1">({auth.account_id})</span>
              )}
            </span>
          </div>
          <button
            onClick={onLogout}
            className="flex items-center gap-1.5 px-3 py-1.5 text-[12px] text-text-2 hover:text-err border border-border rounded-md hover:border-err/30 transition-colors"
          >
            <LogOut size={12} />
            Logout
          </button>
        </div>
      ) : (
        <div>
          <button
            onClick={onLogin}
            disabled={authLoading}
            className="flex items-center gap-2 px-4 py-2 bg-surface-3 border border-border rounded-lg text-[13px] text-text-0 hover:bg-surface-4 transition-colors disabled:opacity-50"
          >
            <ExternalLink size={14} />
            {authLoading ? "Opening browser..." : "Login with OpenAI"}
          </button>
          <p className="text-[11px] text-text-3 mt-2">
            Or enter an API key below
          </p>
        </div>
      )}
    </Section>
  );
}
