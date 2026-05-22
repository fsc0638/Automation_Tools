"use client";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { BarChart3, Bot, CalendarDays, FolderOpen, Lock, LogOut, Map as MapIcon, NotebookPen, Search, ShieldCheck, Target, UserCog } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAuthStore } from "@/lib/store";
import { auth } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { LocaleSwitcher } from "@/components/LocaleSwitcher";

export function Sidebar() {
  const pathname = usePathname();
  const router = useRouter();
  const { user, logout } = useAuthStore();
  const t = useT();

  const nav = [
    { href: "/projects", label: t("sidebar.projects"), description: t("sidebar.projectsDesc"), icon: FolderOpen },
    { href: "/meetings", label: t("sidebar.meetings"), description: t("sidebar.meetingsDesc"), icon: CalendarDays },
    { href: "/roadmap", label: t("sidebar.globalRoadmap"), description: t("sidebar.globalRoadmapDesc"), icon: MapIcon },
    { href: "/insights", label: t("sidebar.globalInsights"), description: t("sidebar.globalInsightsDesc"), icon: BarChart3 },
    { href: "/search", label: t("sidebar.globalSearch"), description: t("sidebar.globalSearchDesc"), icon: Search },
    { href: "/epics", label: t("sidebar.epics"), description: t("sidebar.epicsDesc"), icon: Target },
    { href: "/memory", label: t("sidebar.sharedMemory"), description: t("sidebar.sharedMemoryDesc"), icon: NotebookPen },
    { href: "/agents", label: t("sidebar.agents"), description: t("sidebar.agentsDesc"), icon: UserCog },
    // /access is the project ACL / org-workspace surface added by the
    // openclaw merge — Hermes never saw it, so the label stays English
    // until i18n keys land. Slot it at the end so existing muscle memory
    // for the top entries stays intact.
    { href: "/access", label: "Access", description: "Organizations, workspaces, and sharing", icon: ShieldCheck },
    { href: "/vault", label: t("sidebar.vault"), description: t("sidebar.vaultDesc"), icon: Lock },
  ];

  function handleLogout() {
    void auth.logout();
    logout();
    router.push("/login");
  }

  return (
    <aside className="flex h-full w-[17.5rem] flex-shrink-0 flex-col border-r border-white/10 bg-[radial-gradient(circle_at_top,_rgba(58,126,204,0.18),_transparent_38%),linear-gradient(180deg,#061936_0%,#08142B_100%)] text-white">
      <div className="flex-shrink-0 border-b border-white/10 px-5 py-5">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-2xl bg-white/10 shadow-[0_12px_30px_rgba(0,0,0,0.22)] backdrop-blur-sm">
              <span className="text-sm font-bold text-white">K</span>
            </div>
            <div>
              <div className="text-[15px] font-semibold tracking-[-0.015em] text-white">{t("sidebar.title")}</div>
              <div className="mt-1 text-[13px] leading-5 text-blue-100/72">{t("sidebar.subtitle")}</div>
            </div>
          </div>
          <LocaleSwitcher tone="dark" />
        </div>
        {/* Removed the "Workspace status" promo card — it was a static
            marketing-style summary that duplicated information already
            visible elsewhere (Projects list, conversation rail) and only
            took vertical room from the actual nav. */}
      </div>

      {/* `min-h-0` releases the flex child from its intrinsic content
          height so `overflow-y-auto` can engage when nav items grow
          past the viewport. Without min-h-0, flex-1 keeps stretching
          to fit content and pushes the user-info block off-screen. */}
      <nav className="min-h-0 flex-1 overflow-y-auto px-3 py-4">
        <div className="mb-2 px-3 text-[12px] font-semibold tracking-[0.08em] text-blue-100/48">
          {t("sidebar.navigation")}
        </div>
        <div className="space-y-1.5">
          {nav.map(({ href, label, description, icon: Icon }) => {
            const active = pathname.startsWith(href);
            return (
              <Link
                key={href}
                href={href}
                className={cn(
                  "group flex items-start gap-3 rounded-2xl border px-3 py-3 transition-all",
                  active
                    ? "border-[#3A7ECC]/40 bg-white/10 shadow-[0_16px_40px_rgba(0,0,0,0.18)]"
                    : "border-transparent text-blue-100/74 hover:border-white/10 hover:bg-white/6 hover:text-white"
                )}
              >
                <div className={cn(
                  "mt-0.5 rounded-xl p-2",
                  active ? "bg-[#3A7ECC]/25 text-white" : "bg-white/6 text-blue-100/70 group-hover:text-white"
                )}>
                  <Icon size={15} />
                </div>
                <div className="min-w-0">
                  <div className={cn("text-[14px] font-medium tracking-[-0.01em]", active ? "text-white" : "text-current")}>{label}</div>
                  <div className="mt-1 text-[12px] leading-5 text-blue-100/58">{description}</div>
                </div>
              </Link>
            );
          })}
        </div>
      </nav>

      {/* Pinned user block. `flex-shrink-0` guards against an edge case
          where a flex sibling could squeeze this region; the avatar +
          email + sign-out button should always be visible. */}
      <div className="flex-shrink-0 border-t border-white/10 p-4">
        <div className="rounded-2xl border border-white/10 bg-white/6 p-4 backdrop-blur-sm">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-2xl bg-[#3A7ECC]/25 text-sm font-semibold text-white">
              {user?.display_name?.charAt(0).toUpperCase() ?? "U"}
            </div>
            <div className="min-w-0 flex-1">
              <p className="truncate text-[14px] font-medium tracking-[-0.01em] text-white">{user?.display_name}</p>
              <p className="truncate text-[12px] text-blue-100/62">{user?.email}</p>
            </div>
            <div className="rounded-xl bg-white/8 p-2 text-blue-100/70">
              <Bot size={14} />
            </div>
          </div>
          <button
            onClick={handleLogout}
            className="mt-4 inline-flex w-full items-center justify-center gap-2 rounded-xl border border-white/10 bg-white/6 px-3 py-2.5 text-[14px] font-medium text-blue-100/82 transition hover:border-white/20 hover:bg-white/10 hover:text-white"
            title={t("common.signOut")}
          >
            <LogOut size={15} />
            {t("common.signOut")}
          </button>
        </div>
      </div>
    </aside>
  );
}
