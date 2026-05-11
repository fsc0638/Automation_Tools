"use client";
import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { Sidebar } from "@/components/Sidebar";
import { useAuthStore, useWorkspaceChromeStore } from "@/lib/store";

const IDLE_TIMEOUT_MS = 30 * 60 * 1000;

export default function AppLayout({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const token = useAuthStore((s) => s.token);
  const hasHydrated = useAuthStore((s) => s.hasHydrated);
  const logout = useAuthStore((s) => s.logout);
  const showAppSidebar = useWorkspaceChromeStore((s) => s.showAppSidebar);

  useEffect(() => {
    if (!hasHydrated) return;
    if (!token) router.replace("/login");
  }, [token, hasHydrated, router]);

  useEffect(() => {
    if (!token) return;
    let timerId: ReturnType<typeof setTimeout> | null = null;

    const armTimer = () => {
      if (timerId !== null) clearTimeout(timerId);
      timerId = setTimeout(() => {
        logout();
      }, IDLE_TIMEOUT_MS);
    };

    let lastReset = 0;
    const onActivity = () => {
      const now = Date.now();
      if (now - lastReset < 5_000) return;
      lastReset = now;
      armTimer();
    };

    const events: Array<keyof WindowEventMap> = ["mousedown", "keydown", "touchstart", "scroll", "focus"];
    for (const ev of events) {
      window.addEventListener(ev, onActivity, { passive: true });
    }
    armTimer();

    return () => {
      if (timerId !== null) clearTimeout(timerId);
      for (const ev of events) window.removeEventListener(ev, onActivity);
    };
  }, [token, logout]);

  if (!hasHydrated) return null;
  if (!token) return null;

  return (
    <div className="flex h-screen overflow-hidden bg-[radial-gradient(circle_at_top,_#F8FBFF_0%,_#F5F7FB_34%,_#EEF3F8_100%)]">
      {showAppSidebar && <Sidebar />}
      <main className="min-w-0 flex-1 overflow-auto bg-transparent">{children}</main>
    </div>
  );
}
