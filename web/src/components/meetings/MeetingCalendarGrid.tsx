"use client";
import { useEffect, useMemo, useState } from "react";
import { meetings as meetingsApi, type Meeting, type MeetingCalendarDay } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { buildMonthGrid, isoDate } from "./meeting-utils";

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

export function MeetingCalendarGrid({
  selectedDate,
  onSelectDate,
}: {
  selectedDate: Date;
  onSelectDate: (d: Date) => void;
}) {
  const t = useT();
  const [year, setYear] = useState(selectedDate.getFullYear());
  const [month, setMonth] = useState(selectedDate.getMonth() + 1);
  const [days, setDays] = useState<MeetingCalendarDay[]>([]);
  const [meetingsByDay, setMeetingsByDay] = useState<Map<string, Meeting[]>>(new Map());

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [calendar, list] = await Promise.all([
          meetingsApi.calendar(year, month),
          meetingsApi.list({
            from: new Date(year, month - 1, 1).toISOString(),
            to: new Date(year, month, 1).toISOString(),
          }),
        ]);
        if (cancelled) return;
        setDays(calendar);
        const map = new Map<string, Meeting[]>();
        for (const m of list) {
          const key = isoDate(new Date(m.start_at));
          const arr = map.get(key) ?? [];
          arr.push(m);
          map.set(key, arr);
        }
        setMeetingsByDay(map);
      } catch {
        if (!cancelled) {
          setDays([]);
          setMeetingsByDay(new Map());
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [year, month]);

  const grid = useMemo(() => buildMonthGrid(year, month), [year, month]);
  const dayInfo = useMemo(() => {
    const m = new Map<string, MeetingCalendarDay>();
    for (const d of days) m.set(d.date, d);
    return m;
  }, [days]);

  const monthLabel = `${year} 年 ${String(month).padStart(2, "0")} 月`;

  const totalMeetings = days.reduce((s, d) => s + d.meeting_count, 0);
  const availableSlots = days.filter((d) => d.has_available_slot).length;

  function shiftMonth(delta: number) {
    let newMonth = month + delta;
    let newYear = year;
    if (newMonth < 1) {
      newMonth = 12;
      newYear -= 1;
    }
    if (newMonth > 12) {
      newMonth = 1;
      newYear += 1;
    }
    setMonth(newMonth);
    setYear(newYear);
  }

  const todayIso = isoDate(new Date());
  const selectedIso = isoDate(selectedDate);

  return (
    <section className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <header className="mb-4 flex items-center justify-between">
        <div>
          <div className="text-[18px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
            {t("meetings.calendarTitle")}
          </div>
          <div className="mt-1 text-[12px] text-[#94A3B8]">{t("meetings.calendarDesc")}</div>
        </div>
        <div className="flex items-center gap-2 text-[12px] text-[#475569]">
          <button onClick={() => shiftMonth(-1)} className="rounded-lg border border-[#E2E8F0] px-2.5 py-1 hover:bg-[#F8FAFC]">
            ‹
          </button>
          <span className="min-w-[120px] text-center font-medium text-[#1A1A2E]">{monthLabel}</span>
          <button onClick={() => shiftMonth(1)} className="rounded-lg border border-[#E2E8F0] px-2.5 py-1 hover:bg-[#F8FAFC]">
            ›
          </button>
        </div>
      </header>

      <div className="mb-3 text-right text-[12px] text-[#94A3B8]">
        {t("meetings.calendar.meetingCount").replace("{n}", String(totalMeetings))} ·{" "}
        {t("meetings.calendar.openSlots").replace("{n}", String(availableSlots))}
      </div>

      <div className="grid grid-cols-7 gap-1 text-center text-[11px] font-medium text-[#94A3B8]">
        {WEEKDAYS.map((w) => (
          <div key={w} className="py-1.5">{w}</div>
        ))}
      </div>

      <div className="mt-1 grid grid-cols-7 gap-1">
        {grid.map((g) => {
          const iso = isoDate(g.date);
          const info = dayInfo.get(iso);
          const meetings = meetingsByDay.get(iso) ?? [];
          const isSelected = iso === selectedIso;
          const isToday = iso === todayIso;
          return (
            <button
              key={iso}
              type="button"
              onClick={() => onSelectDate(g.date)}
              className={cn(
                "flex h-[88px] flex-col items-stretch rounded-lg border p-1.5 text-left transition",
                isSelected
                  ? "border-[#0050A0] bg-[#EFF6FF]"
                  : isToday
                    ? "border-[#94A3B8] bg-[#F8FAFC] hover:bg-white"
                    : g.isCurrentMonth
                      ? "border-[#E2E8F0] bg-white hover:bg-[#F8FAFC]"
                      : "border-[#F1F5F9] bg-[#FBFCFE] text-[#CBD5E1] hover:bg-[#F1F5F9]"
              )}
            >
              <div className={cn("text-[12px] font-semibold", isSelected ? "text-[#0050A0]" : g.isCurrentMonth ? "text-[#1A1A2E]" : "text-[#CBD5E1]")}>
                {g.dayOfMonth}
              </div>

              {/* Dots row */}
              <div className="mt-1 flex gap-0.5">
                {info?.has_urgent && <span className="h-1.5 w-1.5 rounded-full bg-[#F59E0B]" />}
                {info && info.meeting_count > 0 && (
                  <span className="h-1.5 w-1.5 rounded-full bg-[#0050A0]" />
                )}
                {info?.has_available_slot && (
                  <span className="h-1.5 w-1.5 rounded-full bg-[#10B981]" />
                )}
              </div>

              {/* First meeting time preview */}
              {meetings[0] && (
                <div className="mt-auto truncate rounded bg-[#EFF6FF] px-1 py-0.5 text-[10px] text-[#0050A0]">
                  {new Date(meetings[0].start_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false })}{" "}
                  {meetings[0].title.slice(0, 6)}
                </div>
              )}
            </button>
          );
        })}
      </div>

      <div className="mt-4 flex items-center gap-4 text-[11px] text-[#64748B]">
        <span className="flex items-center gap-1.5"><span className="h-2 w-2 rounded-full bg-[#10B981]" />{t("meetings.calendar.legend.available")}</span>
        <span className="flex items-center gap-1.5"><span className="h-2 w-2 rounded-full bg-[#0050A0]" />{t("meetings.calendar.legend.scheduled")}</span>
        <span className="flex items-center gap-1.5"><span className="h-2 w-2 rounded-full bg-[#F59E0B]" />{t("meetings.calendar.legend.urgent")}</span>
      </div>
      <div className="mt-2 text-[12px] leading-5 text-[#94A3B8]">{t("meetings.calendar.legendHint")}</div>
    </section>
  );
}
