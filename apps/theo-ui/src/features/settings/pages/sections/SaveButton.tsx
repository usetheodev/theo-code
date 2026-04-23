import { Check } from "lucide-react";
import { clsx } from "clsx";

export interface SaveButtonProps {
  saving: boolean;
  saved: boolean;
  onSave: () => Promise<void>;
}

export function SaveButton({ saving, saved, onSave }: SaveButtonProps) {
  return (
    <div className="flex justify-end pt-2">
      <button
        onClick={onSave}
        disabled={saving}
        className={clsx(
          "flex items-center gap-2 px-5 py-2 rounded-lg text-[13px] font-medium transition-all",
          saved
            ? "bg-ok/10 text-ok border border-ok/20"
            : "bg-brand text-white hover:bg-brand-hover",
          saving && "opacity-50",
        )}
      >
        {saved && <Check size={14} />}
        {saved ? "Saved" : saving ? "Saving..." : "Save settings"}
      </button>
    </div>
  );
}
