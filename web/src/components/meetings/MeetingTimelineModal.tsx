"use client";
/* eslint-disable react-hooks/set-state-in-effect */
import { useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { X } from "lucide-react";
import { meetings as meetingsApi, type Meeting } from "@/lib/api";
import { cn } from "@/lib/utils";
import { formatDateOnly } from "./meeting-utils";

/**
 * Horizontal day-timeline of all meetings on a given date.
 *
 * Rendered as a fullscreen overlay so the workbench's month/list context
 * stays underneath, untouched. The user can ESC / click-outside / press the
 * close button to return.
 *
 * Layout:
 *   - X axis: hours from VIEW_START to VIEW_END (extended when bookings
 *     fall outside, so off-hours meetings are not silently clipped).
 *   - Y axis: one row per location ("No location" row when missing).
 *   - Each meeting = absolutely-positioned bar inside its row, coloured by
 *     status / importance. Click jumps to the meeting detail page.
 *
 * Cancelled meetings are still shown so the user can see "this slot WAS
 * planned then was cancelled" — but rendered muted + struck-through.
 */
const VIEW_START_HOUR_DEFAULT = 8;
const VIEW_END_HOUR_DEFAULT = 21; // exclusive: last tick is 20:00
const ROW_HEIGHT = 48;
const ROW_GAP = 8;

export function MeetingTimelineModal({
  date,
  open,
  onClose,
}: {
  date: Date | null;
  open: boolean;
  onClose: () => void;
}) {
  const router = useRouter();
  const [meetings, setMeetings] = useState<Meeting[]>([]);
  // Union of locations seen in a wider window — keeps the room rows stable
  // even when the selected day has zero bookings for some rooms.
  const [knownLocations, setKnownLocations] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  // Re-render every minute so the "now" indicator line moves with the
  // clock. The interval is cheap (a useState bump) and only runs while
  // the modal is open.
  const [, setNowTick] = useState(0);
  useEffect(() => {
    if (!open) return;
    const id = window.setInterval(() => setNowTick((n) => n + 1), 60_000);
    return () => window.clearInterval(id);
  }, [open]);

  // Fetch whenever the modal becomes visible with a date.
  // Two queries run in parallel: the target day (drives the bars) and a
  // ±60-day window (drives the row list). That way a brand-new empty day
  // still shows every room the user has ever booked.
  useEffect(() => {
    if (!open || !date) return;
    let cancelled = false;
    setLoading(true);
    setError("");
    (async () => {
      try {
        const dayStart = new Date(date);
        dayStart.setHours(0, 0, 0, 0);
        const dayEnd = new Date(dayStart);
        dayEnd.setDate(dayStart.getDate() + 1);

        const wideStart = new Date(dayStart);
        wideStart.setDate(dayStart.getDate() - 60);
        const wideEnd = new Date(dayStart);
        wideEnd.setDate(dayStart.getDate() + 60);

        const [todayList, wideList] = await Promise.all([
          meetingsApi.list({
            from: dayStart.toISOString(),
            to: dayEnd.toISOString(),
          }),
          meetingsApi.list({
            from: wideStart.toISOString(),
            to: wideEnd.toISOString(),
          }),
        ]);
        if (cancelled) return;
        // Hide cancelled bookings — they're audit-trail rows from when the
        // portal re-keyed a booking (e.g. someone changed the time slot),
        // not live meetings the timeline cares about. DB still holds them.
        setMeetings(todayList.filter((m) => m.status !== "cancelled"));
        const set = new Set<string>();
        for (const m of wideList) {
          if (m.status === "cancelled") continue;
          const k = (m.location ?? "").trim();
          if (k) set.add(k);
        }
        setKnownLocations(Array.from(set));
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : "讀取失敗");
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, date]);

  // ESC closes the modal.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // Compute the effective hour window. Default 08:00-21:00, but widen if
  // any booking sits outside so its bar isn't silently clipped.
  const { startHour, endHour, rows } = useMemo(() => {
    let minHour = VIEW_START_HOUR_DEFAULT;
    let maxHour = VIEW_END_HOUR_DEFAULT;
    for (const m of meetings) {
      const s = new Date(m.start_at);
      const e = new Date(m.end_at);
      minHour = Math.min(minHour, s.getHours());
      // Round end up to the next hour so a 09:30 end isn't truncated to 09:00.
      const endRounded = e.getMinutes() > 0 ? e.getHours() + 1 : e.getHours();
      maxHour = Math.max(maxHour, endRounded);
    }
    minHour = Math.max(0, Math.min(minHour, VIEW_START_HOUR_DEFAULT));
    maxHour = Math.min(24, Math.max(maxHour, VIEW_END_HOUR_DEFAULT));

    // Build the row set from (known locations ∪ today's locations) so empty
    // rooms still appear as blank rows. Today's bookings get bucketed under
    // their matching location; "(未指定地點)" only shows up when something
    // is actually scheduled without a location.
    const byLocation = new Map<string, Meeting[]>();
    for (const loc of knownLocations) byLocation.set(loc, []);
    for (const m of meetings) {
      const key = (m.location ?? "").trim() || "(未指定地點)";
      const arr = byLocation.get(key) ?? [];
      arr.push(m);
      byLocation.set(key, arr);
    }
    const sortedRows = Array.from(byLocation.entries())
      .map(([location, list]) => ({
        location,
        meetings: list.sort(
          (a, b) =>
            new Date(a.start_at).getTime() - new Date(b.start_at).getTime()
        ),
      }))
      .sort((a, b) => {
        if (a.location === "(未指定地點)") return 1;
        if (b.location === "(未指定地點)") return -1;
        return a.location.localeCompare(b.location, "zh-TW");
      });

    return { startHour: minHour, endHour: maxHour, rows: sortedRows };
  }, [meetings, knownLocations]);

  if (!open || !date) return null;

  const totalMinutes = (endHour - startHour) * 60;
  const hourTicks: number[] = [];
  for (let h = startHour; h <= endHour; h++) hourTicks.push(h);

  // "Now" indicator: only render when the modal is viewing today (the
  // user might be inspecting a future or past day, where a "now" line is
  // meaningless). Position is the same fraction we use for booking bars.
  const now = new Date();
  const isToday =
    now.getFullYear() === date.getFullYear() &&
    now.getMonth() === date.getMonth() &&
    now.getDate() === date.getDate();
  const nowFraction = isToday
    ? ((now.getHours() * 60 + now.getMinutes()) - startHour * 60) / totalMinutes
    : -1;
  const showNow = isToday && nowFraction >= 0 && nowFraction <= 1;
  const nowLeft = `${(nowFraction * 100).toFixed(3)}%`;
  const nowLabel = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;

  function barPosition(m: Meeting): { left: string; width: string } {
    const s = new Date(m.start_at);
    const e = new Date(m.end_at);
    const startMin = (s.getHours() - startHour) * 60 + s.getMinutes();
    const endMin = (e.getHours() - startHour) * 60 + e.getMinutes();
    const left = Math.max(0, startMin) / totalMinutes;
    const width = Math.max(0.005, Math.min(1, endMin / totalMinutes) - left);
    return { left: `${(left * 100).toFixed(3)}%`, width: `${(width * 100).toFixed(3)}%` };
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="flex h-full max-h-[88vh] w-full max-w-[1280px] flex-col overflow-hidden rounded-2xl bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b border-[#E2E8F0] bg-white px-6 py-4">
          <div>
            <div className="text-[18px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
              {formatDateOnly(date.toISOString())} 會議時間軸
            </div>
            <div className="mt-1 text-[12px] text-[#94A3B8]">
              {loading
                ? "讀取中…"
                : `${meetings.length} 場會議 · ${rows.length} 個地點`}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="關閉時間軸"
            className="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-[#E2E8F0] bg-white text-[#475569] hover:bg-[#F8FAFC] hover:text-[#1A1A2E]"
          >
            <X size={16} />
          </button>
        </header>

        <div className="flex-1 overflow-auto p-6">
          {loading ? (
            <div className="flex h-full items-center justify-center text-[13px] text-[#94A3B8]">
              讀取中…
            </div>
          ) : error ? (
            <div className="flex h-full items-center justify-center text-[13px] text-[#C8102E]">
              {error}
            </div>
          ) : rows.length === 0 ? (
            <div className="flex h-full items-center justify-center text-[13px] text-[#94A3B8]">
              尚無任何會議室紀錄。先建立會議或從 KWay portal 匯入資料。
            </div>
          ) : (
            <div className="relative min-w-[800px]">
              {/* Hour scale */}
              <div className="grid" style={{ gridTemplateColumns: "160px 1fr" }}>
                <div />
                <div className="relative h-7 border-b border-[#E2E8F0]">
                  {hourTicks.map((h) => {
                    const pos = ((h - startHour) / (endHour - startHour)) * 100;
                    return (
                      <div
                        key={h}
                        className="absolute -translate-x-1/2 text-[11px] text-[#94A3B8]"
                        style={{ left: `${pos.toFixed(3)}%` }}
                      >
                        {String(h).padStart(2, "0")}:00
                      </div>
                    );
                  })}
                  {showNow && (
                    <div
                      className="pointer-events-none absolute -translate-x-1/2 whitespace-nowrap rounded-md bg-[#C8102E] px-1.5 py-0.5 text-[10px] font-semibold text-white shadow-sm"
                      style={{ left: nowLeft, top: 0 }}
                    >
                      {nowLabel}
                    </div>
                  )}
                </div>
              </div>

              {/* Rows */}
              <div className="mt-2 space-y-2">
                {rows.map((row) => (
                  <div
                    key={row.location}
                    className="grid items-stretch"
                    style={{ gridTemplateColumns: "160px 1fr" }}
                  >
                    <div className="flex items-center pr-3 text-[12px] font-medium text-[#1A1A2E]">
                      <span className="truncate" title={row.location}>{row.location}</span>
                    </div>
                    <div
                      className="relative rounded-lg border border-[#E2E8F0] bg-[#F8FAFC]"
                      style={{ height: ROW_HEIGHT, marginBottom: ROW_GAP }}
                    >
                      {/* Vertical hour gridlines */}
                      {hourTicks.map((h) => {
                        const pos = ((h - startHour) / (endHour - startHour)) * 100;
                        return (
                          <div
                            key={h}
                            className="absolute top-0 h-full w-px bg-[#E2E8F0]"
                            style={{ left: `${pos.toFixed(3)}%` }}
                          />
                        );
                      })}
                      {/* (The "now" marker is rendered once as a full-
                          height overlay on the outer wrapper below, so it
                          spans the entire grid without per-row breaks.) */}

                      {row.meetings.map((m) => {
                        const pos = barPosition(m);
                        // Bar text is just the booker's name; room name is
                        // already on the left axis and time on the top axis.
                        // We parse off the trailing " · 姓名" the importer
                        // writes ("1號會議室(8人) · 黃若瑀"); for in-app
                        // meetings without that pattern, fall back to title.
                        const personLabel = extractPerson(m.title);
                        return (
                          <button
                            key={m.id}
                            type="button"
                            onClick={() => router.push(`/meetings/${m.id}`)}
                            className={cn(
                              "absolute top-1.5 bottom-1.5 flex items-center overflow-hidden rounded-md border px-2 py-1 text-left text-[11px] font-medium transition hover:brightness-105",
                              barColor(m)
                            )}
                            style={{ left: pos.left, width: pos.width }}
                            title={`${m.title}\n${fmtTimeRange(m)}`}
                          >
                            <span className="truncate">{personLabel}</span>
                          </button>
                        );
                      })}
                    </div>
                  </div>
                ))}
              </div>

              {/* Single full-height "now" marker overlay. Positioned at
                  160px (label column width) + fraction × remaining width,
                  so it lines up with the hour ticks inside the timeline
                  column. Spans from below the hour-scale (28px = h-7)
                  through to the bottom of the last row, with no per-row
                  break. 60% opacity + z-20 so it sits over the booking
                  bars without hiding their text completely. */}
              {showNow && (
                <>
                  {/* Line center sits at `calc(...)`, matching the dot
                      below, so the cap dot stays exactly under the line
                      regardless of nowFraction. */}
                  <div
                    className="pointer-events-none absolute bottom-0 w-0.5 -translate-x-1/2 bg-[#C8102E] z-20"
                    style={{
                      top: 28,
                      left: `calc(160px + ${nowFraction} * (100% - 160px))`,
                      opacity: 0.4,
                    }}
                  />
                  {/* Small dot finishing the bottom of the now-line. Same
                      center coordinate as the line + translateY(50%) so
                      the dot's middle sits on the line's bottom edge. */}
                  <div
                    className="pointer-events-none absolute h-2 w-2 -translate-x-1/2 translate-y-1/2 rounded-full bg-[#C8102E] z-20"
                    style={{
                      bottom: 0,
                      left: `calc(160px + ${nowFraction} * (100% - 160px))`,
                    }}
                  />
                </>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function extractPerson(title: string): string {
  // Importer format: "<room> · <user>". Take the last segment.
  const parts = title.split(" · ");
  return parts.length > 1 ? parts[parts.length - 1] : title;
}

function fmtHm(iso: string): string {
  const d = new Date(iso);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function fmtTimeRange(m: Meeting): string {
  return `${fmtHm(m.start_at)} - ${fmtHm(m.end_at)}`;
}

function barColor(m: Meeting): string {
  if (m.status === "cancelled") {
    return "border-[#CBD5E1] bg-[#F1F5F9] text-[#94A3B8] line-through";
  }
  if (m.status === "completed") {
    return "border-[#A7F3D0] bg-[#ECFDF5] text-[#065F46]";
  }
  if (m.importance === "important") {
    return "border-[#FCA5A5] bg-[#FEE2E2] text-[#991B1B]";
  }
  return "border-[#BFDBFE] bg-[#EFF6FF] text-[#1E40AF]";
}
