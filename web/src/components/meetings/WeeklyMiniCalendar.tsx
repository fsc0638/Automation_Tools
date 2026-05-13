"use client";
import { useEffect, useMemo, useState } from "react";
import { meetings as meetingsApi, type MeetingCalendarDay } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { isoDate } from "./meeting-utils";

const WEEKDAYS_FULL = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/**
 * 7-day mini calendar shown on the create-meeting page right panel. Days
 * highlight the currently selected day; busy and meeting-count info is
 * sourced from the same /meetings/calendar endpoint used on the workbench.
 */
export function WeeklyMiniCalendar({ selectedDate }: { selectedDate: Date }) {
  const t = useT();
  const [info, setInfo] = useState<Map<string, MeetingCalendarDay>>(new Map());

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const year = selectedDate.getFullYear();
      const month = selectedDate.getMonth() + 1;
      try {
        const list = await meetingsApi.calendar(year, month);
        if (!cancelled) {
          const m = new Map<string, MeetingCalendarDay>();
          for (const d of list) m.set(d.date, d);
          setInfo(m);
        }
      } catch {
        if (!cancelled) setInfo(new Map());
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selectedDate]);

  // Week starts Monday containing selectedDate.
  const week = useMemo(() => {
    const base = new Date(selectedDate);
    const dow = base.getDay(); // 0=Sun
    const offsetToMonday = (dow + 6) % 7;
    base.setDate(base.getDate() - offsetToMonday);
    return Array.from({ length: 7 }, (_, i) => {
      const d = new Date(base);
      d.setDate(base.getDate() + i);
      return d;
    });
  }, [selectedDate]);

  const todayIso = isoDate(new Date());
  const selectedIso = isoDate(selectedDate);

  return (
    <div className="grid grid-cols-7 gap-1.5">
      {week.map((d, i) => {
        const iso = isoDate(d);
        const dayInfo = info.get(iso);
        const isSelected = iso === selectedIso;
        const isToday = iso === todayIso;
        const count = dayInfo?.meeting_count ?? 0;
        const busy = count >= 5;
        return (
          <div
            key={iso}
            className={cn(
              "rounded-lg border p-2 text-center",
              isSelected
                ? "border-[#0050A0] bg-[#EFF6FF]"
                : isToday
                  ? "border-[#94A3B8] bg-white"
                  : "border-[#E2E8F0] bg-white"
            )}
          >
            <div className="text-[11px] font-medium text-[#94A3B8]">{WEEKDAYS_FULL[i]}</div>
            <div className={cn("mt-1 text-[18px] font-semibold tracking-[-0.01em]", isSelected ? "text-[#0050A0]" : "text-[#1A1A2E]")}>
              {d.getDate()}
            </div>
            <div className="mt-1.5 text-[10px] text-[#94A3B8]">
              {busy ? t("meetings.schedule.status.busy") : count > 0 ? `${count} 場` : "0 場"}
            </div>
          </div>
        );
      })}
    </div>
  );
}
