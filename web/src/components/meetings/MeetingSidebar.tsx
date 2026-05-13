"use client";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { Search } from "lucide-react";
import { meetings as meetingsApi, type Meeting, type MeetingStatus } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { formatDateOnly, formatTimeRange } from "./meeting-utils";

type TabKey = "recent" | "drafts" | "history";

const TAB_STATUS: Record<TabKey, MeetingStatus | undefined> = {
  recent: "scheduled",
  drafts: "draft",
  history: "completed",
};

export function MeetingSidebar({
  activeMeetingId,
}: {
  activeMeetingId?: string;
}) {
  const t = useT();
  const [tab, setTab] = useState<TabKey>("recent");
  const [items, setItems] = useState<Meeting[]>([]);
  const [search, setSearch] = useState("");

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await meetingsApi.list({ status: TAB_STATUS[tab] });
        if (!cancelled) setItems(list);
      } catch {
        if (!cancelled) setItems([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [tab]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return items;
    return items.filter((m) =>
      [m.title, m.location ?? "", m.notification_note ?? ""]
        .some((s) => s.toLowerCase().includes(q))
    );
  }, [items, search]);

  const upcoming = filtered.slice(0, 5);

  return (
    <aside className="flex h-full w-[280px] flex-shrink-0 flex-col gap-4 overflow-y-auto border-r border-[#E2E8F0] bg-white p-5">
      <div>
        <div className="text-[18px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
          {t("meetings.title")}
        </div>
        <div className="mt-1 text-[12px] text-[#94A3B8]">
          {t("meetings.subtitle")}
        </div>
      </div>

      <Link
        href="/meetings/new"
        className="block rounded-xl bg-[#1A1A2E] py-2.5 text-center text-[13px] font-medium text-white transition hover:bg-[#243149]"
      >
        {t("meetings.createBtn")}
      </Link>

      <div className="relative">
        <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-[#94A3B8]" />
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t("meetings.searchPlaceholder")}
          className="w-full rounded-xl border border-[#E2E8F0] bg-white py-2 pl-9 pr-3 text-[13px] placeholder:text-[#94A3B8] focus:border-[#0050A0] focus:outline-none"
        />
      </div>

      <div className="flex gap-1 rounded-xl bg-[#F1F5F9] p-1 text-[12px]">
        {(["recent", "drafts", "history"] as const).map((k) => (
          <button
            key={k}
            onClick={() => setTab(k)}
            className={cn(
              "flex-1 rounded-lg py-1.5 transition",
              tab === k
                ? "bg-white text-[#1A1A2E] shadow-sm"
                : "text-[#64748B] hover:text-[#1A1A2E]"
            )}
          >
            {t(`meetings.tabs.${k}`)}
          </button>
        ))}
      </div>

      {/* AI reminder card */}
      <div className="rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] p-3">
        <div className="text-[12px] font-semibold text-[#1A1A2E]">
          {t("meetings.aiReminderTitle")}
        </div>
        <div className="mt-1.5 text-[12px] leading-5 text-[#475569]">
          {t("meetings.aiReminderBody").replace("{count}", String(items.filter((m) => m.status === "completed").length || 0))}
        </div>
      </div>

      <div>
        <div className="text-[12px] font-semibold uppercase tracking-[0.06em] text-[#94A3B8]">
          {t("meetings.upcomingTitle")}
        </div>
        <div className="mt-2 space-y-2">
          {upcoming.length === 0 ? (
            <div className="rounded-lg border border-dashed border-[#E2E8F0] px-3 py-4 text-center text-[12px] text-[#94A3B8]">
              {t("meetings.list.empty")}
            </div>
          ) : (
            upcoming.map((m) => (
              <Link
                key={m.id}
                href={`/meetings/${m.id}`}
                className={cn(
                  "block rounded-xl border px-3 py-3 transition",
                  activeMeetingId === m.id
                    ? "border-[#0050A0] bg-[#EFF6FF]"
                    : "border-[#E2E8F0] bg-white hover:border-[#CBD5E1]"
                )}
              >
                <div className="text-[13px] font-semibold text-[#1A1A2E]">{m.title}</div>
                <div className="mt-1 text-[11px] text-[#94A3B8]">
                  {formatDateOnly(m.start_at)} · {formatTimeRange(m.start_at, m.end_at)}
                </div>
                {m.location && (
                  <div className="mt-1.5">
                    <span className="inline-block rounded-md bg-[#F1F5F9] px-2 py-0.5 text-[11px] text-[#475569]">
                      {m.location.split(" / ")[0]}
                    </span>
                  </div>
                )}
              </Link>
            ))
          )}
        </div>
      </div>
    </aside>
  );
}
