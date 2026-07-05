import { getReview } from "@/lib/tauri";
import type { Review as ReviewType, Severity } from "@/types";
import { AlertCircle, CheckCircle, Loader2, RefreshCw, XCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";

export function Review() {
  const [review, setReview] = useState<ReviewType | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    setLoading(true);
    try {
      const r = await getReview();
      setReview(r);
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
        <h1 className="text-2xl font-semibold">Policy Review</h1>
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
          Loading review...
        </div>
      )}

      {!loading && !review && !error && (
        <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-8 text-center">
          <p className="text-[var(--fg-muted)]">No review loaded yet.</p>
          <Button variant="secondary" onClick={load} className="mt-4">
            Load Review
          </Button>
        </div>
      )}

      {review && (
        <>
          <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
            <p className="text-sm text-[var(--fg-muted)]">Portfolio value</p>
            <p className="text-2xl font-semibold">
              {formatCurrency(review.portfolioValue, review.currency)}
            </p>
            <p className="mt-1 text-sm text-[var(--fg-muted)]">Policy: {review.policyName}</p>
          </div>

          {review.findings.length > 0 && (
            <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
              <h2 className="mb-4 text-lg font-medium">Findings</h2>
              <div className="space-y-3">
                {review.findings.map((f, i) => (
                  <div
                    key={i}
                    className={`rounded-lg border border-[var(--border)] p-4 ${
                      f.severity === "Critical"
                        ? "bg-[var(--danger-bg)]"
                        : f.severity === "Warning"
                          ? "bg-[var(--warning-bg)]"
                          : "bg-[var(--success-bg)]"
                    }`}
                  >
                    <div className="flex items-center gap-2">
                      <SeverityIcon severity={f.severity} />
                      <span className="text-xs font-medium uppercase text-[var(--fg-muted)]">
                        {f.severity}
                      </span>
                      <span className="text-xs text-[var(--fg-muted)]">· {f.category}</span>
                    </div>
                    <p className="mt-1 text-sm font-medium">{f.message}</p>
                    {f.detail && <p className="mt-1 text-xs text-[var(--fg-muted)]">{f.detail}</p>}
                  </div>
                ))}
              </div>
            </div>
          )}

          <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
            <h2 className="mb-4 text-lg font-medium">Allocation vs Target</h2>
            <table className="w-full text-sm">
              <thead className="text-left text-[var(--fg-muted)]">
                <tr>
                  <th className="pb-2">Asset Class</th>
                  <th className="pb-2 text-right">Target</th>
                  <th className="pb-2 text-right">Actual</th>
                  <th className="pb-2 text-right">Drift</th>
                  <th className="pb-2 text-right">Status</th>
                </tr>
              </thead>
              <tbody>
                {review.allocations.map((a) => (
                  <tr key={a.assetClass} className="border-t border-[var(--border)]">
                    <td className="py-2">{a.assetClass}</td>
                    <td className="py-2 text-right">{a.targetPercent.toFixed(1)}%</td>
                    <td className="py-2 text-right">{a.actualPercent.toFixed(1)}%</td>
                    <td className="py-2 text-right">{a.driftPercent.toFixed(1)}%</td>
                    <td className="py-2 text-right">
                      {a.withinTolerance ? (
                        <CheckCircle className="inline h-4 w-4 text-[var(--success)]" />
                      ) : (
                        <XCircle className="inline h-4 w-4 text-[var(--danger)]" />
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {review.suggestedActions.length > 0 && (
            <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
              <h2 className="mb-3 text-lg font-medium">Suggested Actions</h2>
              <ul className="list-inside list-disc space-y-1 text-sm text-[var(--fg-muted)]">
                {review.suggestedActions.map((a, i) => (
                  <li key={i}>{a}</li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function SeverityIcon({ severity }: { severity: Severity }) {
  if (severity === "Critical") return <XCircle className="h-4 w-4 text-[var(--danger)]" />;
  if (severity === "Warning") return <AlertCircle className="h-4 w-4 text-[var(--warning)]" />;
  return <CheckCircle className="h-4 w-4 text-[var(--success)]" />;
}
