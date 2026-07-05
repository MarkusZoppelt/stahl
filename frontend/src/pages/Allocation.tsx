import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import type { AllocationItem, PortfolioSummary } from "@/types";
import { Cell, Pie, PieChart, ResponsiveContainer, Tooltip } from "recharts";

interface AllocationProps {
  summary: PortfolioSummary | null;
  allocation: AllocationItem[];
  currency: string;
}

const COLORS = [
  "#7aa2f7",
  "#73daca",
  "#bb9af7",
  "#e0af68",
  "#7dcfff",
  "#f7768e",
  "#9ece6a",
  "#ff9e64",
];

function formatCurrency(value: number, currency: string): string {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(value);
}

export function Allocation({ summary, allocation, currency }: AllocationProps) {
  const totalValue = summary?.totalValue ?? 0;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Allocation</h1>
        <p className="text-[var(--fg-muted)]">Asset class breakdown</p>
      </div>

      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>By Asset Class</CardTitle>
            <CardDescription>Percentage of total portfolio</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="h-80">
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie
                    data={allocation}
                    dataKey="percent"
                    nameKey="assetClass"
                    cx="50%"
                    cy="50%"
                    innerRadius={70}
                    outerRadius={110}
                    paddingAngle={2}
                  >
                    {allocation.map((_, index) => (
                      <Cell key={`cell-${index}`} fill={COLORS[index % COLORS.length]} />
                    ))}
                  </Pie>
                  <Tooltip
                    contentStyle={{
                      backgroundColor: "var(--bg-secondary)",
                      borderColor: "var(--border)",
                      color: "var(--fg)",
                    }}
                    formatter={(value: number) => [`${value.toFixed(2)}%`, "Allocation"]}
                  />
                </PieChart>
              </ResponsiveContainer>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Breakdown</CardTitle>
            <CardDescription>Value by asset class</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              {allocation.map((item, index) => (
                <div
                  key={item.assetClass}
                  className="flex items-center justify-between rounded-lg border border-[var(--border)] bg-[var(--bg)] p-3"
                >
                  <div className="flex items-center gap-3">
                    <div
                      className="h-3 w-3 rounded-full"
                      style={{ backgroundColor: COLORS[index % COLORS.length] }}
                    />
                    <span className="font-medium">{item.assetClass}</span>
                  </div>
                  <div className="flex items-center gap-4">
                    <span className="text-[var(--fg-muted)]">
                      {formatCurrency(item.value, currency)}
                    </span>
                    <Badge variant="info">{item.percent.toFixed(2)}%</Badge>
                  </div>
                </div>
              ))}
              {allocation.length === 0 && (
                <p className="text-sm text-[var(--fg-muted)]">No allocation data.</p>
              )}
            </div>
            <div className="mt-4 flex justify-between border-t border-[var(--border)] pt-3 text-sm font-medium">
              <span>Total</span>
              <span>{formatCurrency(totalValue, currency)}</span>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
