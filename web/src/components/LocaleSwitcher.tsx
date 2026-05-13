"use client";
import { useEffect, useRef, useState } from "react";
import { Globe, Check } from "lucide-react";
import { useLocaleStore, LOCALES, type Locale } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/**
 * Globe-icon dropdown for choosing the UI language. Selecting a locale
 * persists it to localStorage (via the zustand persist middleware) and then
 * forces a full page reload so every component picks up the new dictionary
 * — simpler than threading reactive state through the entire UI tree.
 */
export function LocaleSwitcher({ tone = "dark" }: { tone?: "dark" | "light" }) {
  const locale = useLocaleStore((s) => s.locale);
  const setLocale = useLocaleStore((s) => s.setLocale);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function onClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    if (open) document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, [open]);

  function pick(next: Locale) {
    if (next === locale) {
      setOpen(false);
      return;
    }
    setLocale(next);
    setOpen(false);
    // Reload so SSR-rendered text and any non-reactive references are updated.
    if (typeof window !== "undefined") window.location.reload();
  }

  const current = LOCALES.find((l) => l.value === locale) ?? LOCALES[0];

  const triggerClass = tone === "dark"
    ? "bg-white/6 text-white/90 hover:bg-white/12 border-white/10"
    : "bg-white text-[#1A1A2E] hover:bg-[#F8FAFC] border-[#E2E8F0]";

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        suppressHydrationWarning
        className={cn(
          "flex items-center gap-2 rounded-xl border px-3 py-2 text-xs font-medium transition",
          triggerClass,
        )}
        title="Language / 語言"
      >
        <Globe size={14} />
        <span className="truncate">{current.native}</span>
      </button>
      {open && (
        <div
          className={cn(
            "absolute z-50 mt-2 min-w-[180px] overflow-hidden rounded-xl border shadow-lg",
            tone === "dark"
              ? "border-white/10 bg-[#0c1f3d] text-white"
              : "border-[#E2E8F0] bg-white text-[#1A1A2E]",
          )}
        >
          {LOCALES.map((l) => {
            const active = l.value === locale;
            return (
              <button
                key={l.value}
                type="button"
                onClick={() => pick(l.value)}
                className={cn(
                  "flex w-full items-center justify-between gap-3 px-3 py-2 text-sm transition",
                  tone === "dark"
                    ? active
                      ? "bg-[#3A7ECC]/30"
                      : "hover:bg-white/8"
                    : active
                      ? "bg-[#EFF6FF] text-[#0050A0]"
                      : "hover:bg-[#F8FAFC]",
                )}
              >
                <div className="flex flex-col items-start">
                  <span className="font-medium">{l.native}</span>
                  <span className={cn("text-[10px]", tone === "dark" ? "text-white/55" : "text-[#94A3B8]")}>
                    {l.label}
                  </span>
                </div>
                {active && <Check size={14} />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
