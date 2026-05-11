"use client";
import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { Sidebar } from "@/components/Sidebar";
import { useAuthStore, useWorkspaceChromeStore } from "@/lib/store";

/** 30 minutes — any real user activity (mousedown / keydown / touch /
 *  scroll) resets the timer. After 30 idle minutes we call logout(),
 *  which clears the token; the redirect-to-login effect picks it up. */
const IDLE_TIMEOUT_MS = 30 * 60 * 1000;

export default function AppLayout({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const token = useAuthStore((s) => s.token);
  const hasHydrated = useAuthStore((s) => s.hasHydrated);
  const logout = useAuthStore((s) => s.logout);
  const showAppSidebar = useWorkspaceChromeStore((s) => s.showAppSidebar);

  // Only check token after persist has finished reading localStorage —
  // otherwise the SSR / first-paint window sees the default `null` and
  // bounces the user to /login even on a valid session.
  useEffect(() => {
    if (!hasHydrated) return;
    if (!token) router.replace("/login");
  }, [token, hasHydrated, router]);

  // 30-minute idle timeout. Resets on any user activity. Skipped while
  // unauthenticated — no point running it on /login.
  useEffect(() => {
    if (!token) return;
    let timerId: ReturnType<typeof setTimeout> | null = null;

    const armTimer = () => {
      if (timerId !== null) clearTimeout(timerId);
      timerId = setTimeout(() => {
        logout();
      }, IDLE_TIMEOUT_MS);
    };

    // Throttle: never reset more than once per 5 seconds. mousedown /
    // keydown / scroll can fire dozens of times per second; we don't
    // need to recompute the timeout every tick.
    let lastReset = 0;
    const onActivity = () => {
      const now = Date.now();
      if (now - lastReset < 5_000) return;
      lastReset = now;
      armTimer();
    };

    const events: Array<keyof WindowEventMap> = [
      "mousedown",
      "keydown",
      "touchstart",
      "scroll",
      "focus",
    ];
    for (const ev of events) {
      window.addEventListener(ev, onActivity, { passive: true });
    }
    armTimer();

    return () => {
      if (timerId !== null) clearTimeout(timerId);
      for (const ev of events) window.removeEventListener(ev, onActivity);
    };
  }, [token, logout]);

  // Hold the first paint until hydration completes — render nothing
  // (instead of `/login`-flash → real-page-flash) for a clean reload.
  if (!hasHydrated) return null;
  if (!token) return null;

  return (
    <div className="flex h-screen overflow-hidden bg-[#F8F9FA]">
      {showAppSidebar && <Sidebar />}
      <main className="min-w-0 flex-1 overflow-auto bg-[#F8F9FA]">{children}</main>
    </div>
  );
}
