import { getSimulation } from "@/lib/tauri";
import type { RebalanceSimulation, RebalanceScenario } from "@/types";
import { Loader2, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";

export function Simulate() {
  const [sim, setSim] = useState<RebalanceSimulation | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    setLoading(true);
    try {
      const s = await getSimulation();
      setSim(s);
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
        <h1 className="text-2xl font-semibold">Rebalance Simulation</h1>
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
          Loading simulation...
        </div>
      )}

      {!loading && !sim && !error && (
        <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-8 text-center">
          <p className="text-[var(--fg-muted)]">No simulation loaded yet.</p>
          <Button variant="secondary" onClick={load} className="mt-4">
            Load Simulation
          </Button>
        </div>
      )}

      {sim && (
        <>
          <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
            <p className="text-sm text-[var(--fg-muted)]">Portfolio value</p>
            <p className="text-2xl font-semibold">
              {formatCurrency(sim.portfolioValue, sim.currency)}
            </p>
            <p className="mt-1 text-sm text-[var(--fg-muted)]">Policy: {sim.policyName}</p>
          </div>

          <div className="grid gap-6 lg:grid-cols-2">
            {sim.scenarios.map((scenario) => (
              <ScenarioCard
                key={scenario.name}
                scenario={scenario}
                currency={sim.currency}
                formatCurrency={formatCurrency}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function ScenarioCard({
  scenario,
  currency,
  formatCurrency,
}: {
  scenario: RebalanceScenario;
  currency: string;
  formatCurrency: (value: number, currency: string) => string;
}) {
  return (
    <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
      <h2 className="text-lg font-medium">{scenario.name}</h2>
      <p className="text-sm text-[var(--fg-muted)]">{scenario.description}</p>

      {scenario.trades.length === 0 ? (
        <p className="mt-4 text-sm text-[var(--success)]">No trades needed.</p>
      ) : (
        <table className="mt-4 w-full text-sm">
          <thead className="text-left text-[var(--fg-muted)]">
            <tr>
              <th className="pb-2">Asset Class</th>
              <th className="pb-2">Action</th>
              <th className="pb-2 text-right">Amount</th>
              <th className="pb-2 text-right">% of Portfolio</th>
            </tr>
          </thead>
          <tbody>
            {scenario.trades.map((t, i) => (
              <tr key={i} className="border-t border-[var(--border)]">
                <td className="py-2">{t.assetClass}</td>
                <td className="py-2">
                  <span
                    className={`rounded px-2 py-0.5 text-xs ${
                      t.action === "Reduce"
                        ? "bg-[var(--danger-bg)] text-[var(--danger)]"
                        : "bg-[var(--success-bg)] text-[var(--success)]"
                    }`}
                  >
                    {t.action}
                  </span>
                </td>
                <td className="py-2 text-right">{formatCurrency(t.amount, currency)}</td>
                <td className="py-2 text-right">{t.percentOfPortfolio.toFixed(1)}%</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
