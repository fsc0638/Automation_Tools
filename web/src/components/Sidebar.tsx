"use client";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { FolderOpen, MessageSquare, LogOut, Settings, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAuthStore } from "@/lib/store";

const nav = [
  { href: "/projects", label: "Projects", icon: FolderOpen },
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
    <aside className="w-60 flex-shrink-0 bg-[#001A42] flex flex-col h-full">
      {/* Logo */}
      <div className="h-16 flex items-center px-5 border-b border-white/10">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-[#0050A0] flex items-center justify-center">
            <span className="text-white font-bold text-sm">K</span>
          </div>
          <span className="text-white font-semibold text-base">Kway Dev</span>
        </div>
      </div>

      {/* Nav */}
      <nav className="flex-1 px-3 py-4 flex flex-col gap-1">
        {nav.map(({ href, label, icon: Icon }) => (
          <Link
            key={href}
            href={href}
            className={cn(
              "flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm transition-colors",
              pathname.startsWith(href)
                ? "bg-[#0050A0] text-white"
                : "text-[#94A3B8] hover:text-white hover:bg-white/10"
            )}
          >
            <Icon size={16} />
            {label}
          </Link>
        ))}
      </nav>

      {/* User */}
      <div className="border-t border-white/10 p-3">
        <div className="flex items-center gap-3 px-3 py-2.5 rounded-lg">
          <div className="w-8 h-8 rounded-full bg-[#0050A0] flex items-center justify-center flex-shrink-0">
            <span className="text-white text-xs font-semibold">
              {user?.display_name?.charAt(0).toUpperCase() ?? "U"}
            </span>
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-white text-sm font-medium truncate">{user?.display_name}</p>
            <p className="text-[#64748B] text-xs truncate">{user?.email}</p>
          </div>
          <button
            onClick={handleLogout}
            className="text-[#64748B] hover:text-white transition-colors"
            title="Sign out"
          >
            <LogOut size={15} />
          </button>
        </div>
      </div>
    </aside>
  );
}
