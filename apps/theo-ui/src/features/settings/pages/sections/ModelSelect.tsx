export interface ModelSelectProps {
  models: string[];
  value: string;
  onChange: (v: string) => void;
  loading: boolean;
}

export function ModelSelect({ models, value, onChange, loading }: ModelSelectProps) {
  if (loading && models.length === 0) {
    return (
      <label className="flex flex-col gap-1">
        <span className="text-[11px] text-text-3 font-medium">Model</span>
        <div className="px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-3">
          Loading models...
        </div>
      </label>
    );
  }
  if (models.length === 0) {
    return (
      <label className="flex flex-col gap-1">
        <span className="text-[11px] text-text-3 font-medium">Model</span>
        <input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder="model name"
          className="px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-0 placeholder:text-text-3 outline-none focus:border-border-focus transition-colors"
        />
      </label>
    );
  }
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[11px] text-text-3 font-medium">Model</span>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-0 outline-none focus:border-border-focus transition-colors"
      >
        {models.map((m) => (
          <option key={m} value={m}>
            {m}
          </option>
        ))}
      </select>
    </label>
  );
}
