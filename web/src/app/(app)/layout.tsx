"use client";
import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { Sidebar } from "@/components/Sidebar";
import { useAuthStore, useWorkspaceChromeStore } from "@/lib/store";

export default function AppLayout({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const token = useAuthStore((s) => s.token);
  const showAppSidebar = useWorkspaceChromeStore((s) => s.showAppSidebar);

  useEffect(() => {
    if (!token) router.replace("/login");
  }, [token, router]);

  if (!token) return null;

  return (
    <div className="flex h-screen overflow-hidden bg-[#F8F9FA]">
      {showAppSidebar && <Sidebar />}
      <main className="min-w-0 flex-1 overflow-auto bg-[#F8F9FA]">{children}</main>
    </div>
  );
}
