"use client";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { RefreshCw, Search } from "lucide-react";
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
  refreshKey = 0,
  onAfterRefresh,
}: {
  activeMeetingId?: string;
  /** Parent-driven bump triggers a sidebar re-fetch (e.g. after the
   *  workbench refreshes its calendar). */
  refreshKey?: number;
  /** Called after a successful manual sync click so the parent can
   *  re-fetch its own data. */
  onAfterRefresh?: () => void;
}) {
  const t = useT();
  const [tab, setTab] = useState<TabKey>("recent");
  const [items, setItems] = useState<Meeting[]>([]);
  const [search, setSearch] = useState("");
  const [syncing, setSyncing] = useState(false);
  const [lastSyncedAt, setLastSyncedAt] = useState<Date | null>(null);
  const [syncError, setSyncError] = useState<string>("");

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        // "近期會議" is intentionally narrow — today + tomorrow only — so
        // the sidebar shows what the user actually needs to act on now.
        // Drafts / history use status filtering and no date window.
        const query: Parameters<typeof meetingsApi.list>[0] = {
          status: TAB_STATUS[tab],
        };
        if (tab === "recent") {
          // Start from today 00:00 so meetings that started this morning
          // and are still in progress remain available; the per-meeting
          // `end_at > now` filter below is what actually hides finished
          // ones. End at day-after-tomorrow 00:00 to keep the list short.
          const start = new Date();
          start.setHours(0, 0, 0, 0);
          const end = new Date(start);
          end.setDate(start.getDate() + 2);
          query.from = start.toISOString();
          query.to = end.toISOString();
        }
        let list = await meetingsApi.list(query);
        if (tab === "recent") {
          // Hide meetings whose end_at has already passed — they're "done
          // for today" and don't need to dominate the upcoming list. Sort
          // ascending so today comes before tomorrow.
          const now = Date.now();
          list = [...list]
            .filter((m) => new Date(m.end_at).getTime() > now)
            .sort(
              (a, b) =>
                new Date(a.start_at).getTime() - new Date(b.start_at).getTime()
            );
        }
        if (!cancelled) setItems(list);
      } catch {
        if (!cancelled) setItems([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [tab, refreshKey]);

  async function handleManualSync() {
    if (syncing) return;
    setSyncing(true);
    setSyncError("");
    try {
      await meetingsApi.sync();
      setLastSyncedAt(new Date());
      onAfterRefresh?.();
    } catch (e) {
      setSyncError(e instanceof Error ? e.message : "同步失敗");
    } finally {
      setSyncing(false);
    }
  }

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return items;
    return items.filter((m) =>
      [m.title, m.location ?? "", m.notification_note ?? ""]
        .some((s) => s.toLowerCase().includes(q))
    );
  }, [items, search]);

  // For the today+tomorrow window we show everything (the list is already
  // bounded by the date filter). For drafts/history we cap to keep the
  // sidebar from turning into a long scroll.
  const upcoming = tab === "recent" ? filtered : filtered.slice(0, 5);

  return (
    <aside className="flex h-full w-[280px] flex-shrink-0 flex-col gap-4 overflow-y-auto border-r border-[#E2E8F0] bg-white p-5">
      <div>
        <div className="flex items-center justify-between gap-2">
          <div className="text-[18px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
            {t("meetings.title")}
          </div>
          <button
            type="button"
            onClick={() => void handleManualSync()}
            disabled={syncing}
            aria-label="重新整理會議資料"
            title={
              syncing
                ? "同步中…"
                : lastSyncedAt
                  ? `上次同步：${lastSyncedAt.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
                  : "重新整理會議資料"
            }
            className={cn(
              "inline-flex h-7 w-7 items-center justify-center rounded-full border border-[#E2E8F0] bg-white text-[#475569] transition",
              syncing ? "opacity-60" : "hover:bg-[#F8FAFC] hover:text-[#0050A0]"
            )}
          >
            <RefreshCw size={13} className={cn(syncing && "animate-spin")} />
          </button>
        </div>
        <div className="mt-1 text-[12px] text-[#94A3B8]">
          {t("meetings.subtitle")}
        </div>
        {syncError && (
          <div className="mt-2 rounded-md bg-[#FEE2E2] px-2 py-1 text-[11px] text-[#991B1B]">
            {syncError}
          </div>
        )}
        {!syncError && lastSyncedAt && (
          <div className="mt-1 text-[11px] text-[#94A3B8]">
            上次同步：{lastSyncedAt.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
          </div>
        )}
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
            upcoming.map((m) => {
              // visibility='busy' marks "this isn't your meeting" but
              // per 2026-05-15 brief — "非自己建立的會議應該要可以看到"
              // — non-creator rows are still rendered with real title
              // and remain clickable to the read-only detail page.
              const isOther = m.visibility === "busy";
              return (
                <Link
                  key={m.id}
                  href={`/meetings/${m.id}`}
                  className={cn(
                    "block rounded-xl border px-3 py-3 transition",
                    activeMeetingId === m.id
                      ? "border-[#0050A0] bg-[#EFF6FF]"
                      : isOther
                        ? "border-[#E2E8F0] bg-[#F8FAFC] hover:border-[#CBD5E1]"
                        : "border-[#E2E8F0] bg-white hover:border-[#CBD5E1]"
                  )}
                >
                  <div className="flex items-center gap-2">
                    <div className={cn(
                      "min-w-0 flex-1 truncate text-[13px] font-semibold",
                      isOther ? "text-[#475569]" : "text-[#1A1A2E]"
                    )}>
                      {m.title}
                    </div>
                    {isOther && (
                      <span className="shrink-0 rounded-md border border-[#E2E8F0] bg-white px-1.5 py-0.5 text-[10px] text-[#64748B]">
                        檢視
                      </span>
                    )}
                  </div>
                  <div className="mt-1 text-[11px] text-[#94A3B8]">
                    {formatDateOnly(m.start_at)} · {formatTimeRange(m.start_at, m.end_at)}
                  </div>
                  {(m.location || m.creator_name) && (
                    <div className="mt-1.5 flex items-center justify-between gap-2">
                      {m.location ? (
                        <span className="inline-block truncate rounded-md bg-[#F1F5F9] px-2 py-0.5 text-[11px] text-[#475569]">
                          {m.location.split(" / ")[0]}
                        </span>
                      ) : (
                        <span />
                      )}
                      {m.creator_name && (
                        <span className="shrink-0 text-[11px] text-[#94A3B8]">
                          {m.creator_name}
                        </span>
                      )}
                    </div>
                  )}
                </Link>
              );
            })
          )}
        </div>
      </div>
    </aside>
  );
}
