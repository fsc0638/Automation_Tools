"use client";
import { useState } from "react";
import { Search } from "lucide-react";
import { MeetingSidebar } from "@/components/meetings/MeetingSidebar";
import { MeetingCalendarGrid } from "@/components/meetings/MeetingCalendarGrid";
import { MeetingListPanel } from "@/components/meetings/MeetingListPanel";
import { MeetingTimelineModal } from "@/components/meetings/MeetingTimelineModal";
import { useT } from "@/lib/i18n";

export default function MeetingsWorkbenchPage() {
  const t = useT();
  const [selectedDate, setSelectedDate] = useState<Date>(() => new Date());
  const [search, setSearch] = useState("");
  // Double-clicking a day cell opens the horizontal-timeline modal for
  // that date. Null = modal closed. The modal is portal-like (renders
  // over the page) so the workbench's month/list selection stays intact.
  const [timelineDate, setTimelineDate] = useState<Date | null>(null);
  // Bumped after the sidebar's manual refresh finishes; children depend on
  // it so they re-fetch fresh data alongside the sidebar.
  const [refreshKey, setRefreshKey] = useState(0);

  const monthLabel = `${selectedDate.toLocaleString("en", { month: "long" })} ${selectedDate.getFullYear()}`;

  return (
    <div className="flex h-screen overflow-hidden">
      <MeetingSidebar
        refreshKey={refreshKey}
        onAfterRefresh={() => setRefreshKey((k) => k + 1)}
      />

      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
        {/* Top bar: search + week export. Create-meeting CTA lives in the
            left sidebar to avoid duplicate entry points. */}
        <header className="flex items-center gap-4 border-b border-[#E2E8F0] bg-white px-6 py-3">
          <div className="relative w-[480px] max-w-full">
            <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-[#94A3B8]" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("meetings.searchPlaceholder")}
              className="w-full rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] py-2 pl-9 pr-3 text-[13px] placeholder:text-[#94A3B8] focus:border-[#0050A0] focus:bg-white focus:outline-none"
            />
          </div>
          <div className="flex-1" />
          <button className="rounded-xl border border-[#E2E8F0] bg-white px-3.5 py-2 text-[13px] font-medium text-[#475569] hover:bg-[#F8FAFC]">
            匯出週報
          </button>
        </header>

        {/* Main */}
        <div className="flex min-h-0 flex-1 gap-5 overflow-auto p-6">
          <section className="flex-1 space-y-4">
            <header className="flex items-end justify-between">
              <div>
                <h1 className="text-[22px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
                  {t("meetings.workbenchTitle")}
                </h1>
                <p className="mt-1 text-[13px] text-[#94A3B8]">{t("meetings.workbenchDesc")}</p>
              </div>
              <span className="text-[12px] text-[#94A3B8]">{monthLabel}</span>
            </header>

            <MeetingCalendarGrid
              selectedDate={selectedDate}
              onSelectDate={setSelectedDate}
              onOpenTimeline={(d) => setTimelineDate(d)}
              refreshKey={refreshKey}
            />
          </section>

          <MeetingListPanel date={selectedDate} refreshKey={refreshKey} />
        </div>
      </div>

      <MeetingTimelineModal
        date={timelineDate}
        open={timelineDate !== null}
        onClose={() => setTimelineDate(null)}
      />
    </div>
  );
}
