import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import type { PortfolioSummary } from "@/types";
import {
  ArrowDown,
  ArrowUp,
  Briefcase,
  DollarSign,
  PiggyBank,
  TrendingUp,
  Wallet,
} from "lucide-react";

interface DashboardProps {
  summary: PortfolioSummary | null;
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

export function Dashboard({ summary, currency }: DashboardProps) {
  if (!summary) {
    return (
      <div className="flex h-full flex-col items-center justify-center text-[var(--fg-muted)]">
        <Wallet className="mb-4 h-12 w-12 opacity-20" />
        <p className="text-lg font-medium">No portfolio loaded</p>
        <p className="text-sm">Open a portfolio file to get started.</p>
      </div>
    );
  }

  const stats = [
    {
      label: "Total Value",
      value: formatCurrency(summary.totalValue, currency),
      change: formatPercent(summary.dayChangePercent),
      icon: DollarSign,
      isPositive: summary.dayChangePercent >= 0,
    },
    {
      label: "Securities",
      value: formatCurrency(summary.securitiesValue, currency),
      icon: Briefcase,
    },
    {
      label: "Cash",
      value: formatCurrency(summary.cashValue, currency),
      icon: PiggyBank,
    },
    {
      label: "Total PnL",
      value: formatCurrency(summary.totalPnl, currency),
      change: formatPercent(summary.pnlPercent),
      icon: TrendingUp,
      isPositive: summary.pnlPercent >= 0,
    },
  ];

  const topMovers = [...summary.positions]
    .filter((p) => p.dayChangePercent !== null && p.dayChangePercent !== 0)
    .sort((a, b) => Math.abs(b.dayChangePercent ?? 0) - Math.abs(a.dayChangePercent ?? 0))
    .slice(0, 5);

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Dashboard</h1>
        <p className="text-[var(--fg-muted)]">Overview of your portfolio</p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {stats.map((stat) => {
          const Icon = stat.icon;
          return (
            <Card key={stat.label}>
              <CardHeader className="pb-2">
                <CardDescription>{stat.label}</CardDescription>
              </CardHeader>
              <CardContent>
                <div className="flex items-baseline justify-between">
                  <span className="text-2xl font-bold">{stat.value}</span>
                  <Icon className="h-5 w-5 text-[var(--fg-subtle)]" />
                </div>
                {stat.change !== undefined && (
                  <div
                    className={`mt-2 flex items-center text-xs font-medium ${
                      stat.isPositive ? "text-[var(--success)]" : "text-[var(--danger)]"
                    }`}
                  >
                    {stat.isPositive ? (
                      <ArrowUp className="mr-1 h-3 w-3" />
                    ) : (
                      <ArrowDown className="mr-1 h-3 w-3" />
                    )}
                    {stat.change} today
                  </div>
                )}
              </CardContent>
            </Card>
          );
        })}
      </div>

      <div className="grid gap-6 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle>Positions</CardTitle>
            <CardDescription>Your holdings by current value</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              {summary.positions.slice(0, 8).map((position) => (
                <div
                  key={position.id}
                  className="flex items-center justify-between rounded-lg border border-[var(--border)] bg-[var(--bg)] p-3"
                >
                  <div>
                    <p className="font-medium">{position.name || position.ticker || "Unknown"}</p>
                    <p className="text-xs text-[var(--fg-muted)]">
                      {position.assetClass}
                      {position.ticker && ` · ${position.ticker}`}
                    </p>
                  </div>
                  <div className="text-right">
                    <p className="font-medium">{formatCurrency(position.value, currency)}</p>
                    {position.dayChangePercent !== null && (
                      <p
                        className={`text-xs ${
                          position.dayChangePercent >= 0
                            ? "text-[var(--success)]"
                            : "text-[var(--danger)]"
                        }`}
                      >
                        {formatPercent(position.dayChangePercent)}
                      </p>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Top Movers</CardTitle>
            <CardDescription>Biggest daily changes</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              {topMovers.length === 0 && (
                <p className="text-sm text-[var(--fg-muted)]">No price movements today.</p>
              )}
              {topMovers.map((position) => (
                <div
                  key={position.id}
                  className="flex items-center justify-between rounded-lg border border-[var(--border)] bg-[var(--bg)] p-3"
                >
                  <div>
                    <p className="font-medium text-sm">
                      {position.name || position.ticker || "Unknown"}
                    </p>
                  </div>
                  <Badge variant={(position.dayChangePercent ?? 0) >= 0 ? "success" : "danger"}>
                    {formatPercent(position.dayChangePercent ?? 0)}
                  </Badge>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
