import { Button } from "@/components/ui/button";
import {
  generatePolicyFromStrategy,
  getPolicy,
  loadPolicy,
  savePolicy,
  setPolicyFromToml,
} from "@/lib/tauri";
import type { Policy as PolicyType } from "@/types";
import { FileCheck, RefreshCw, Save } from "lucide-react";
import { useEffect, useState } from "react";

const strategies = [
  { value: "balanced-growth", label: "Balanced Growth" },
  { value: "capital-preservation", label: "Capital Preservation" },
  { value: "aggressive-growth", label: "Aggressive Growth" },
  { value: "custom", label: "Custom" },
];

interface PolicyProps {
  workspaceDir: string | null;
}

export function Policy({ workspaceDir }: PolicyProps) {
  const [policy, setPolicy] = useState<PolicyType | null>(null);
  const [toml, setToml] = useState("");
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = async () => {
    setLoading(true);
    try {
      const p = await getPolicy();
      setPolicy(p);
      if (p) {
        setToml(await policyToToml(p));
      }
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
  }, [workspaceDir]);

  const handleLoad = async () => {
    setLoading(true);
    try {
      const p = await loadPolicy();
      setPolicy(p);
      setToml(await policyToToml(p));
      setMessage("Policy loaded");
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleGenerate = async (strategy: string) => {
    setLoading(true);
    try {
      const p = await generatePolicyFromStrategy(strategy);
      setPolicy(p);
      setToml(await policyToToml(p));
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleValidate = async () => {
    setLoading(true);
    try {
      await setPolicyFromToml(toml);
      setMessage("Policy is valid");
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleSave = async () => {
    setLoading(true);
    try {
      const p = await savePolicy(toml);
      setPolicy(p);
      setMessage("Policy saved");
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Investment Policy</h1>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={refresh} disabled={loading}>
            <RefreshCw className="mr-2 h-4 w-4" />
            Refresh
          </Button>
          <Button onClick={handleLoad} disabled={loading || !workspaceDir}>
            <FileCheck className="mr-2 h-4 w-4" />
            Load from Workspace
          </Button>
        </div>
      </div>

      {!policy && (
        <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
          <p className="mb-4 text-[var(--fg-muted)]">Start from a strategy template:</p>
          <div className="flex flex-wrap gap-2">
            {strategies.map((s) => (
              <Button key={s.value} variant="secondary" onClick={() => handleGenerate(s.value)}>
                {s.label}
              </Button>
            ))}
          </div>
        </div>
      )}

      <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-4">
        <textarea
          value={toml}
          onChange={(e) => setToml(e.target.value)}
          className="h-96 w-full resize-none rounded-lg border border-[var(--border)] bg-[var(--bg)] p-4 font-mono text-sm text-[var(--fg)] focus:border-[var(--accent)] focus:outline-none"
          placeholder="# policy.toml"
        />
        <div className="mt-4 flex gap-2">
          <Button variant="secondary" onClick={handleValidate} disabled={loading}>
            Validate
          </Button>
          <Button onClick={handleSave} disabled={loading || !workspaceDir}>
            <Save className="mr-2 h-4 w-4" />
            Save to Workspace
          </Button>
        </div>
      </div>

      {policy && (
        <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
          <h2 className="mb-2 text-lg font-medium">{policy.name}</h2>
          <p className="text-sm text-[var(--fg-muted)]">
            {policy.riskProfile} · {policy.timeHorizonYears} years · {policy.baseCurrency}
          </p>
          <div className="mt-4 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {policy.allocations.map((a) => (
              <div
                key={a.assetClass}
                className="rounded-lg border border-[var(--border)] bg-[var(--bg)] p-3"
              >
                <p className="text-sm font-medium">{a.assetClass}</p>
                <p className="text-xs text-[var(--fg-muted)]">
                  Target {a.targetPercent}%
                  {a.tolerancePercent !== undefined && ` ±${a.tolerancePercent}%`}
                </p>
              </div>
            ))}
          </div>
        </div>
      )}

      {message && (
        <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-tertiary)] px-4 py-3 text-sm">
          {message}
        </div>
      )}
    </div>
  );
}

async function policyToToml(policy: PolicyType): Promise<string> {
  let out = `version = "${policy.version}"\n`;
  out += `name = "${policy.name}"\n`;
  out += `base_currency = "${policy.baseCurrency}"\n`;
  out += `time_horizon_years = ${policy.timeHorizonYears}\n`;
  out += `risk_profile = "${policy.riskProfile}"\n\n`;

  out += "[constraints]\n";
  if (policy.constraints.minimumCashMonths !== undefined) {
    out += `minimum_cash_months = ${policy.constraints.minimumCashMonths}\n`;
  }
  if (policy.constraints.minimumCashAmount !== undefined) {
    out += `minimum_cash_amount = ${policy.constraints.minimumCashAmount}\n`;
  }
  if (policy.constraints.singlePositionLimitPercent !== undefined) {
    out += `single_position_limit_percent = ${policy.constraints.singlePositionLimitPercent}\n`;
  }
  if (policy.constraints.assetClassLimitPercent !== undefined) {
    out += `asset_class_limit_percent = ${policy.constraints.assetClassLimitPercent}\n`;
  }
  out += "\n";

  for (const a of policy.allocations) {
    out += "[[allocations]]\n";
    out += `asset_class = "${a.assetClass}"\n`;
    out += `target_percent = ${a.targetPercent}\n`;
    if (a.tolerancePercent !== undefined) {
      out += `tolerance_percent = ${a.tolerancePercent}\n`;
    }
    out += "\n";
  }

  return out;
}
