import stahlMark from "@/assets/stahl-mark.svg";
import { FolderOpen, FileJson } from "lucide-react";

interface WelcomeProps {
  onOpenWorkspace: () => void;
  onOpenFile: () => void;
}

export function Welcome({ onOpenWorkspace, onOpenFile }: WelcomeProps) {
  return (
    <div className="flex h-full flex-col items-center justify-center text-center">
      <img src={stahlMark} alt="" className="mb-6 h-16 w-16 rounded-2xl" />
      <h1 className="mb-2 text-3xl font-semibold tracking-tight">Welcome to Stahl Capital</h1>
      <p className="mb-8 max-w-md text-[var(--fg-muted)]">
        Your local-first portfolio manager. Choose how you want to get started.
      </p>

      <div className="grid w-full max-w-2xl gap-4 sm:grid-cols-2">
        <button
          onClick={onOpenWorkspace}
          className="group flex flex-col items-start rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6 text-left transition-colors hover:border-[var(--accent)] hover:bg-[var(--bg-tertiary)]"
        >
          <div className="mb-4 flex h-10 w-10 items-center justify-center rounded-lg bg-[var(--accent)] text-[var(--accent-fg)]">
            <FolderOpen className="h-5 w-5" />
          </div>
          <h2 className="mb-1 text-lg font-medium">Open a workspace</h2>
          <p className="text-sm text-[var(--fg-muted)]">
            Recommended. A workspace holds your portfolio, policy, diary, decisions, and reports.
          </p>
        </button>

        <button
          onClick={onOpenFile}
          className="group flex flex-col items-start rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6 text-left transition-colors hover:border-[var(--accent)] hover:bg-[var(--bg-tertiary)]"
        >
          <div className="mb-4 flex h-10 w-10 items-center justify-center rounded-lg bg-[var(--bg-tertiary)] text-[var(--fg)]">
            <FileJson className="h-5 w-5" />
          </div>
          <h2 className="mb-1 text-lg font-medium">Open a positions file</h2>
          <p className="text-sm text-[var(--fg-muted)]">
            Quick start with just a positions.json file. You can always create a workspace later.
          </p>
        </button>
      </div>
    </div>
  );
}
