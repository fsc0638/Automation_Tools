"use client";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { Bot, FolderOpen, LogOut, Sparkles } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAuthStore } from "@/lib/store";

const nav = [
  {
    href: "/projects",
    label: "Projects",
    description: "Manage repositories and workspaces",
    icon: FolderOpen,
  },
];

export function Sidebar() {
  const pathname = usePathname();
  const router = useRouter();
  const { user, logout } = useAuthStore();

  function handleLogout() {
    logout();
    router.push("/login");
  }

  return (
    <aside className="flex h-full w-72 flex-shrink-0 flex-col border-r border-white/10 bg-[radial-gradient(circle_at_top,_rgba(58,126,204,0.22),_transparent_38%),linear-gradient(180deg,#061936_0%,#08142B_100%)] text-white">
      <div className="border-b border-white/10 px-5 py-5">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-2xl bg-white/10 shadow-[0_12px_30px_rgba(0,0,0,0.22)] backdrop-blur-sm">
            <span className="text-sm font-bold text-white">K</span>
          </div>
          <div>
            <div className="text-sm font-semibold tracking-wide text-white">Kway Dev</div>
            <div className="mt-0.5 text-xs text-blue-100/70">AI-native coding workspace</div>
          </div>
        </div>

        <div className="mt-5 rounded-2xl border border-white/10 bg-white/5 p-4 backdrop-blur-sm">
          <div className="flex items-start gap-3">
            <div className="rounded-xl bg-[#3A7ECC]/20 p-2 text-blue-100">
              <Sparkles size={15} />
            </div>
            <div>
              <div className="text-sm font-medium text-white">Workspace status</div>
              <p className="mt-1 text-xs leading-5 text-blue-100/75">
                Use Projects to manage repo-aware conversations, branch context, and multi-agent workflows in one place.
              </p>
            </div>
          </div>
        </div>
      </div>

      <nav className="flex-1 px-3 py-4">
        <div className="mb-2 px-3 text-[11px] font-semibold uppercase tracking-[0.16em] text-blue-100/45">
          Navigation
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
                    : "border-transparent text-blue-100/70 hover:border-white/10 hover:bg-white/6 hover:text-white"
                )}
              >
                <div className={cn(
                  "mt-0.5 rounded-xl p-2",
                  active ? "bg-[#3A7ECC]/25 text-white" : "bg-white/6 text-blue-100/70 group-hover:text-white"
                )}>
                  <Icon size={15} />
                </div>
                <div className="min-w-0">
                  <div className={cn("text-sm font-medium", active ? "text-white" : "text-current")}>{label}</div>
                  <div className="mt-1 text-xs leading-5 text-blue-100/55">{description}</div>
                </div>
              </Link>
            );
          })}
        </div>
      </nav>

      <div className="border-t border-white/10 p-4">
        <div className="rounded-2xl border border-white/10 bg-white/6 p-4 backdrop-blur-sm">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-2xl bg-[#3A7ECC]/25 text-sm font-semibold text-white">
              {user?.display_name?.charAt(0).toUpperCase() ?? "U"}
            </div>
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium text-white">{user?.display_name}</p>
              <p className="truncate text-xs text-blue-100/60">{user?.email}</p>
            </div>
            <div className="rounded-xl bg-white/8 p-2 text-blue-100/70">
              <Bot size={14} />
            </div>
          </div>
          <button
            onClick={handleLogout}
            className="mt-4 inline-flex w-full items-center justify-center gap-2 rounded-xl border border-white/10 bg-white/6 px-3 py-2.5 text-sm font-medium text-blue-100/80 transition hover:border-white/20 hover:bg-white/10 hover:text-white"
            title="Sign out"
          >
            <LogOut size={15} />
            Sign out
          </button>
        </div>
      </div>
    </aside>
  );
}
