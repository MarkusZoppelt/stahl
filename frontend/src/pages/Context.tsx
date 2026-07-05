import { getContext } from "@/lib/tauri";
import type { PortfolioContext } from "@/types";
import { AlertCircle, Loader2, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";

export function Context() {
  const [ctx, setCtx] = useState<PortfolioContext | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    setLoading(true);
    try {
      const c = await getContext();
      setCtx(c);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  function formatCurrency(value: number, currency: string) {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency,
      maximumFractionDigits: 2,
    }).format(value);
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Portfolio Context</h1>
        <Button variant="secondary" onClick={load} disabled={loading}>
          <RefreshCw className="mr-2 h-4 w-4" />
          Refresh
        </Button>
      </div>

      {error && (
        <div className="rounded-lg border border-[var(--danger)] bg-[var(--danger-bg)] px-4 py-3 text-sm text-[var(--danger)]">
          {error}
        </div>
      )}

      {loading && (
        <div className="flex items-center justify-center rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] py-16 text-sm text-[var(--fg-muted)]">
          <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          Loading context...
        </div>
      )}

      {!loading && !ctx && !error && (
        <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-8 text-center">
          <p className="text-[var(--fg-muted)]">No context loaded yet.</p>
          <Button variant="secondary" onClick={load} className="mt-4">
            Load Context
          </Button>
        </div>
      )}

      {ctx && (
        <>
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <StatCard
              label="Total Value"
              value={formatCurrency(ctx.summary.totalValue, ctx.currency)}
            />
            <StatCard label="Cash" value={formatCurrency(ctx.summary.cashValue, ctx.currency)} />
            <StatCard
              label="Securities"
              value={formatCurrency(ctx.summary.securitiesValue, ctx.currency)}
            />
            <StatCard label="Positions" value={ctx.summary.positionCount.toString()} />
          </div>

          <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
            <h2 className="mb-4 text-lg font-medium">Allocation</h2>
            <table className="w-full text-sm">
              <thead className="text-left text-[var(--fg-muted)]">
                <tr>
                  <th className="pb-2">Asset Class</th>
                  <th className="pb-2 text-right">Value</th>
                  <th className="pb-2 text-right">Weight</th>
                  <th className="pb-2 text-right">Positions</th>
                </tr>
              </thead>
              <tbody>
                {ctx.allocation.map((a) => (
                  <tr key={a.assetClass} className="border-t border-[var(--border)]">
                    <td className="py-2">{a.assetClass}</td>
                    <td className="py-2 text-right">{formatCurrency(a.value, ctx.currency)}</td>
                    <td className="py-2 text-right">{a.percent.toFixed(2)}%</td>
                    <td className="py-2 text-right">{a.positionCount}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {ctx.riskFlags.length > 0 && (
            <div className="rounded-xl border border-[var(--warning)] bg-[var(--warning-bg)] p-6">
              <h2 className="mb-3 flex items-center gap-2 text-lg font-medium text-[var(--warning)]">
                <AlertCircle className="h-5 w-5" />
                Risk Flags
              </h2>
              <ul className="list-inside list-disc space-y-1 text-sm">
                {ctx.riskFlags.map((f, i) => (
                  <li key={i}>{f}</li>
                ))}
              </ul>
            </div>
          )}

          {ctx.dataQualityFlags.length > 0 && (
            <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
              <h2 className="mb-3 text-lg font-medium">Data Quality Flags</h2>
              <ul className="list-inside list-disc space-y-1 text-sm text-[var(--fg-muted)]">
                {ctx.dataQualityFlags.map((f, i) => (
                  <li key={i}>{f}</li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4">
      <p className="text-xs text-[var(--fg-muted)]">{label}</p>
      <p className="mt-1 text-xl font-semibold">{value}</p>
    </div>
  );
}
