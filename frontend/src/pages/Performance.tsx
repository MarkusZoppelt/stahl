import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import type { Performance } from "@/types";
import { ArrowDown, ArrowUp } from "lucide-react";

interface PerformanceProps {
  performance: Performance | null;
  currency: string;
}

function formatCurrency(value: number, currency: string): string {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(value);
}

function formatPercent(value: number): string {
  return `${value >= 0 ? "+" : ""}${value.toFixed(2)}%`;
}

export function PerformancePage({ performance, currency }: PerformanceProps) {
  if (!performance) {
    return (
      <div className="flex h-full items-center justify-center text-[var(--fg-muted)]">
        No performance data available.
      </div>
    );
  }

  const metrics = [
    { label: "Total Value", value: formatCurrency(performance.totalValue, currency) },
    { label: "Invested", value: formatCurrency(performance.invested, currency) },
    { label: "Unrealized PnL", value: formatCurrency(performance.unrealizedPnl, currency) },
    { label: "PnL %", value: formatPercent(performance.pnlPercent) },
    { label: "Day PnL", value: formatCurrency(performance.dayPnl, currency) },
    { label: "Day %", value: formatPercent(performance.dayPercent) },
  ];

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Performance</h1>
        <p className="text-[var(--fg-muted)]">Portfolio returns and top movers</p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {metrics.map((metric) => (
          <Card key={metric.label}>
            <CardHeader className="pb-2">
              <CardDescription>{metric.label}</CardDescription>
            </CardHeader>
            <CardContent>
              <span
                className={`text-2xl font-bold ${
                  metric.value.startsWith("+")
                    ? "text-[var(--success)]"
                    : metric.value.startsWith("-")
                      ? "text-[var(--danger)]"
                      : ""
                }`}
              >
                {metric.value}
              </span>
            </CardContent>
          </Card>
        ))}
      </div>

      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Top Gainers</CardTitle>
            <CardDescription>Best performing positions today</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              {performance.topGainers.length === 0 && (
                <p className="text-sm text-[var(--fg-muted)]">No gainers today.</p>
              )}
              {performance.topGainers.map((mover, idx) => (
                <div
                  key={idx}
                  className="flex items-center justify-between rounded-lg border border-[var(--border)] bg-[var(--bg)] p-3"
                >
                  <span className="font-medium">{mover.name}</span>
                  <div className="flex items-center gap-3">
                    <span className="text-sm text-[var(--fg-muted)]">
                      {formatCurrency(mover.pnl, currency)}
                    </span>
                    <Badge variant="success">
                      <ArrowUp className="mr-1 h-3 w-3" />
                      {formatPercent(mover.percent)}
                    </Badge>
                  </div>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Top Losers</CardTitle>
            <CardDescription>Worst performing positions today</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              {performance.topLosers.length === 0 && (
                <p className="text-sm text-[var(--fg-muted)]">No losers today.</p>
              )}
              {performance.topLosers.map((mover, idx) => (
                <div
                  key={idx}
                  className="flex items-center justify-between rounded-lg border border-[var(--border)] bg-[var(--bg)] p-3"
                >
                  <span className="font-medium">{mover.name}</span>
                  <div className="flex items-center gap-3">
                    <span className="text-sm text-[var(--fg-muted)]">
                      {formatCurrency(mover.pnl, currency)}
                    </span>
                    <Badge variant="danger">
                      <ArrowDown className="mr-1 h-3 w-3" />
                      {formatPercent(mover.percent)}
                    </Badge>
                  </div>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
