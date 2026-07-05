import { validatePortfolio } from "@/lib/tauri";
import type { ValidationReport } from "@/types";
import { AlertCircle, CheckCircle, Loader2, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";

export function Validate() {
  const [report, setReport] = useState<ValidationReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    setLoading(true);
    try {
      const r = await validatePortfolio();
      setReport(r);
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

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Validate Portfolio</h1>
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
          Validating portfolio...
        </div>
      )}

      {!loading && !report && !error && (
        <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-8 text-center">
          <p className="text-[var(--fg-muted)]">No validation report loaded yet.</p>
          <Button variant="secondary" onClick={load} className="mt-4">
            Validate Portfolio
          </Button>
        </div>
      )}

      {report && (
        <>
          <div
            className={`rounded-xl border p-6 ${
              report.valid
                ? "border-[var(--success)] bg-[var(--success-bg)]"
                : "border-[var(--danger)] bg-[var(--danger-bg)]"
            }`}
          >
            <div className="flex items-center gap-3">
              {report.valid ? (
                <CheckCircle className="h-6 w-6 text-[var(--success)]" />
              ) : (
                <AlertCircle className="h-6 w-6 text-[var(--danger)]" />
              )}
              <div>
                <p className="font-medium">
                  {report.valid ? "Portfolio is valid" : "Validation failed"}
                </p>
                <p className="text-sm text-[var(--fg-muted)]">
                  {report.positionCount} positions · {report.errorCount} errors ·{" "}
                  {report.warningCount} warnings
                </p>
              </div>
            </div>
          </div>

          {report.issues.length > 0 && (
            <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
              <h2 className="mb-4 text-lg font-medium">Issues</h2>
              <div className="space-y-2">
                {report.issues.map((issue, i) => (
                  <div
                    key={i}
                    className={`rounded-lg border border-[var(--border)] p-3 ${
                      issue.severity === "Error"
                        ? "bg-[var(--danger-bg)]"
                        : "bg-[var(--warning-bg)]"
                    }`}
                  >
                    <p className="text-sm font-medium">
                      {issue.severity === "Error" ? "❌" : "⚠️"}{" "}
                      {issue.position ? `${issue.position}: ` : ""}
                      {issue.message}
                    </p>
                  </div>
                ))}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
