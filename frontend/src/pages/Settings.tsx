import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  clearRememberedPath,
  getConfig,
  getConfigPath,
  setCurrency,
  setLlmConfig,
  setTheme,
} from "@/lib/tauri";
import type { ThemeMode } from "@/types";
import { Moon, RefreshCw, Save, Sun, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

interface SettingsProps {
  onReset: () => void;
}

export function Settings({ onReset }: SettingsProps) {
  const [currency, setCurrencyValue] = useState("EUR");
  const [providerUrl, setProviderUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [theme, setThemeValue] = useState<ThemeMode>("dark");
  const [configPath, setConfigPath] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = async () => {
    setLoading(true);
    try {
      const c = await getConfig();
      setCurrencyValue(c.currency);
      setProviderUrl(c.llmProviderUrl ?? "");
      setApiKey(c.llmApiKey ?? "");
      setModel(c.llmModel ?? "");
      setThemeValue(c.theme);
      const path = await getConfigPath();
      setConfigPath(path);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const handleSave = async () => {
    try {
      await setCurrency(currency);
      await setTheme(theme);
      await setLlmConfig(
        providerUrl.trim() || undefined,
        apiKey.trim() || undefined,
        model.trim() || undefined,
      );
      document.documentElement.classList.toggle("light", theme === "light");
      setMessage("Settings saved");
      await load();
    } catch (e) {
      setMessage(String(e));
    }
  };

  const handleClearPath = async () => {
    try {
      await clearRememberedPath();
      onReset();
      setMessage("Remembered path cleared");
    } catch (e) {
      setMessage(String(e));
    }
  };

  const handleThemeChange = (mode: ThemeMode) => {
    setThemeValue(mode);
    document.documentElement.classList.toggle("light", mode === "light");
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Settings</h1>
        <Button variant="secondary" onClick={load} disabled={loading}>
          <RefreshCw className="mr-2 h-4 w-4" />
          Refresh
        </Button>
      </div>

      {message && (
        <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-tertiary)] px-4 py-3 text-sm">
          {message}
        </div>
      )}

      <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
        <h2 className="mb-4 text-lg font-medium">General</h2>
        <div className="space-y-4">
          <div>
            <label className="mb-1 block text-sm font-medium">Currency</label>
            <Input
              value={currency}
              onChange={(e) => setCurrencyValue(e.target.value)}
              placeholder="EUR"
              className="max-w-xs"
            />
          </div>

          <div>
            <label className="mb-1 block text-sm font-medium">Theme</label>
            <div className="flex gap-2">
              <Button
                variant={theme === "dark" ? "default" : "secondary"}
                onClick={() => handleThemeChange("dark")}
              >
                <Moon className="mr-2 h-4 w-4" />
                Dark
              </Button>
              <Button
                variant={theme === "light" ? "default" : "secondary"}
                onClick={() => handleThemeChange("light")}
              >
                <Sun className="mr-2 h-4 w-4" />
                Light
              </Button>
            </div>
          </div>
        </div>
      </div>

      <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
        <h2 className="mb-4 text-lg font-medium">AI Provider</h2>
        <p className="mb-4 text-sm text-[var(--fg-muted)]">
          Configure an Anthropic or OpenAI-compatible endpoint so Stahl can act as its own AI
          harness.
        </p>
        <div className="space-y-4">
          <div>
            <label className="mb-1 block text-sm font-medium">Provider URL</label>
            <Input
              value={providerUrl}
              onChange={(e) => setProviderUrl(e.target.value)}
              placeholder="https://api.anthropic.com/v1"
              className="max-w-md"
            />
          </div>
          <div>
            <label className="mb-1 block text-sm font-medium">API Key</label>
            <Input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="sk-..."
              className="max-w-md"
            />
            <p className="mt-1 text-xs text-[var(--fg-muted)]">
              Stored in the local config file (see path below) with owner-only permissions, the same
              convention as <code>~/.aws/credentials</code>.
            </p>
          </div>
          <div>
            <label className="mb-1 block text-sm font-medium">Model</label>
            <Input
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder="claude-sonnet-4-20250514"
              className="max-w-md"
            />
          </div>
        </div>
      </div>

      <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
        <h2 className="mb-4 text-lg font-medium">Data</h2>
        <div className="flex flex-col gap-4 sm:flex-row sm:items-center">
          <Button variant="secondary" onClick={handleClearPath} disabled={loading}>
            <Trash2 className="mr-2 h-4 w-4" />
            Clear Remembered Path
          </Button>
          <p className="text-xs text-[var(--fg-muted)]">Config file: {configPath}</p>
        </div>
      </div>

      <div className="flex justify-end">
        <Button onClick={handleSave} disabled={loading}>
          <Save className="mr-2 h-4 w-4" />
          Save Settings
        </Button>
      </div>
    </div>
  );
}
