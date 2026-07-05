import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { draftDecision, generateReport } from "@/lib/tauri";
import type { GeneratedDocument } from "@/types";
import { FileText, Save } from "lucide-react";
import { useState } from "react";

interface DocumentsProps {
  workspaceDir: string | null;
}

export function Documents({ workspaceDir }: DocumentsProps) {
  const [decisionTitle, setDecisionTitle] = useState("");
  const [generated, setGenerated] = useState<GeneratedDocument | null>(null);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const handleReport = async (dryRun: boolean) => {
    if (!workspaceDir) {
      setMessage("No workspace loaded");
      return;
    }
    setLoading(true);
    try {
      const doc = await generateReport(workspaceDir, dryRun);
      setGenerated(doc);
      setMessage(dryRun ? "Report preview generated" : `Report saved to ${doc.filePath}`);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleDecision = async (dryRun: boolean) => {
    if (!workspaceDir) {
      setMessage("No workspace loaded");
      return;
    }
    setLoading(true);
    try {
      const title = decisionTitle.trim() || undefined;
      const doc = await draftDecision(title, workspaceDir, dryRun);
      setGenerated(doc);
      setMessage(dryRun ? "Decision preview generated" : `Decision saved to ${doc.filePath}`);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Documents</h1>
      </div>

      {!workspaceDir && (
        <div className="rounded-lg border border-[var(--warning)] bg-[var(--warning-bg)] px-4 py-3 text-sm text-[var(--warning)]">
          Load a workspace to generate reports and decision records.
        </div>
      )}

      <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
        <h2 className="mb-4 text-lg font-medium flex items-center gap-2">
          <FileText className="h-5 w-5" />
          Weekly Report
        </h2>
        <p className="mb-4 text-sm text-[var(--fg-muted)]">
          Generate a Markdown weekly report with portfolio status and policy alignment.
        </p>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => handleReport(true)} disabled={loading}>
            Preview
          </Button>
          <Button onClick={() => handleReport(false)} disabled={loading}>
            <Save className="mr-2 h-4 w-4" />
            Generate & Save
          </Button>
        </div>
      </div>

      <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
        <h2 className="mb-4 text-lg font-medium">Decision Record</h2>
        <div className="mb-4">
          <label className="mb-1 block text-sm font-medium">Title</label>
          <Input
            value={decisionTitle}
            onChange={(e) => setDecisionTitle(e.target.value)}
            placeholder="Rebalance Stocks"
            className="max-w-md"
          />
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => handleDecision(true)} disabled={loading}>
            Preview
          </Button>
          <Button onClick={() => handleDecision(false)} disabled={loading}>
            <Save className="mr-2 h-4 w-4" />
            Draft & Save
          </Button>
        </div>
      </div>

      {message && (
        <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-tertiary)] px-4 py-3 text-sm">
          {message}
        </div>
      )}

      {generated && (
        <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
          <p className="mb-2 text-sm font-medium">
            {generated.dryRun ? "Preview" : "Saved"}: {generated.filePath}
          </p>
          <pre className="max-h-96 overflow-auto rounded-lg border border-[var(--border)] bg-[var(--bg)] p-4 text-xs font-mono whitespace-pre-wrap">
            {generated.content}
          </pre>
        </div>
      )}
    </div>
  );
}
