import { Button } from "@/components/ui/button";
import { setTheme } from "@/lib/tauri";
import { Moon, Sun } from "lucide-react";
import { useEffect, useState } from "react";

export function ThemeToggle() {
  const [isDark, setIsDark] = useState(true);

  useEffect(() => {
    const root = document.documentElement;
    setIsDark(!root.classList.contains("light"));
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    if (isDark) {
      root.classList.remove("light");
    } else {
      root.classList.add("light");
    }
  }, [isDark]);

  const toggle = async () => {
    const next = !isDark;
    setIsDark(next);
    try {
      await setTheme(next ? "dark" : "light");
    } catch {
      // ignore persistence errors
    }
  };

  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={toggle}
      title={isDark ? "Switch to light mode" : "Switch to dark mode"}
    >
      {isDark ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
    </Button>
  );
}
