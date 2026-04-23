import type { TokenMetrics } from "../types";

interface Props {
  tokens: TokenMetrics;
}

function fmt(n: number): string {
  if (n === 0) return "0";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return n.toString();
}

export function TokenEconomyPanel({ tokens }: Props) {
  const total =
    tokens.input_tokens
    + tokens.output_tokens
    + tokens.cache_read_tokens
    + tokens.cache_write_tokens
    + tokens.reasoning_tokens;
  const bar = (val: number, color: string) => {
    const pct = total > 0 ? (val / total) * 100 : 0;
    return (
      <div className="flex-1 h-4 bg-surface-3 rounded overflow-hidden flex items-center">
        <div className={`h-4 ${color}`} style={{ width: `${pct}%` }} />
      </div>
    );
  };
  return (
    <div className="bg-surface-2 rounded-xl border border-border p-4" data-testid="token-economy">
      <h3 className="text-[13px] font-semibold text-text-1 mb-3">Token Economy</h3>
      <div className="grid grid-cols-2 gap-4">
        <div>
          <div className="text-[11px] text-text-3 uppercase tracking-wide mb-2">Breakdown</div>
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-3">
              <span className="text-[12px] text-text-2 w-28">Input</span>
              {bar(tokens.input_tokens, "bg-accent-blue/60")}
              <span className="font-mono text-[11px] text-text-1 w-20 text-right">{fmt(tokens.input_tokens)}</span>
            </div>
            <div className="flex items-center gap-3">
              <span className="text-[12px] text-text-2 w-28">Output</span>
              {bar(tokens.output_tokens, "bg-accent-green/60")}
              <span className="font-mono text-[11px] text-text-1 w-20 text-right">{fmt(tokens.output_tokens)}</span>
            </div>
            <div className="flex items-center gap-3">
              <span className="text-[12px] text-text-2 w-28">Cache read</span>
              {bar(tokens.cache_read_tokens, "bg-accent-purple/60")}
              <span className="font-mono text-[11px] text-text-1 w-20 text-right">{fmt(tokens.cache_read_tokens)}</span>
            </div>
            <div className="flex items-center gap-3">
              <span className="text-[12px] text-text-2 w-28">Cache write</span>
              {bar(tokens.cache_write_tokens, "bg-accent-yellow/60")}
              <span className="font-mono text-[11px] text-text-1 w-20 text-right">{fmt(tokens.cache_write_tokens)}</span>
            </div>
            <div className="flex items-center gap-3">
              <span className="text-[12px] text-text-2 w-28">Reasoning</span>
              {bar(tokens.reasoning_tokens, "bg-accent-red/60")}
              <span className="font-mono text-[11px] text-text-1 w-20 text-right">{fmt(tokens.reasoning_tokens)}</span>
            </div>
          </div>
        </div>
        <div>
          <div className="text-[11px] text-text-3 uppercase tracking-wide mb-2">KPIs</div>
          <div className="grid grid-cols-2 gap-3">
            <div className="bg-surface-3 rounded p-3">
              <div className="text-[10px] text-text-3 uppercase">Total tokens</div>
              <div className="text-[18px] font-semibold text-text-1">{fmt(total)}</div>
            </div>
            <div className="bg-surface-3 rounded p-3">
              <div className="text-[10px] text-text-3 uppercase">Cost (USD)</div>
              <div className="text-[18px] font-semibold text-text-1">${tokens.total_cost_usd.toFixed(4)}</div>
            </div>
            <div className="bg-surface-3 rounded p-3">
              <div className="text-[10px] text-text-3 uppercase">Cache hit rate</div>
              <div className={`text-[18px] font-semibold ${tokens.cache_hit_rate > 0.3 ? "text-accent-green" : tokens.cache_hit_rate > 0.1 ? "text-accent-yellow" : "text-accent-red"}`}>
                {(tokens.cache_hit_rate * 100).toFixed(1)}%
              </div>
            </div>
            <div className="bg-surface-3 rounded p-3">
              <div className="text-[10px] text-text-3 uppercase">Tokens / edit</div>
              <div className="text-[18px] font-semibold text-text-1">
                {tokens.tokens_per_successful_edit > 0 ? fmt(tokens.tokens_per_successful_edit) : "—"}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
