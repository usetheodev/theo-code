export interface FieldProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: "text" | "password";
  disabled?: boolean;
}

export function Field({
  label,
  value,
  onChange,
  placeholder,
  type = "text",
  disabled,
}: FieldProps) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[11px] text-text-3 font-medium">{label}</span>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        disabled={disabled}
        className="px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-0 placeholder:text-text-3 outline-none focus:border-border-focus transition-colors disabled:opacity-50"
      />
    </label>
  );
}
