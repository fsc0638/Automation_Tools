import type { MeetingStatus } from "@/lib/api";

export function formatTimeRange(startISO: string, endISO: string): string {
  const s = new Date(startISO);
  const e = new Date(endISO);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(s.getHours())}:${pad(s.getMinutes())} - ${pad(e.getHours())}:${pad(e.getMinutes())}`;
}

export function formatDateOnly(iso: string): string {
  const d = new Date(iso);
  return `${d.getFullYear()}/${String(d.getMonth() + 1).padStart(2, "0")}/${String(d.getDate()).padStart(2, "0")}`;
}

export function formatDateTime(iso: string): string {
  const d = new Date(iso);
  return `${formatDateOnly(iso)} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function isoToLocalDateInput(iso: string): string {
  const d = new Date(iso);
  return `${d.getFullYear()}/${String(d.getMonth() + 1).padStart(2, "0")}/${String(d.getDate()).padStart(2, "0")}`;
}

export function isoToLocalTimeInput(iso: string): string {
  const d = new Date(iso);
  const h = d.getHours();
  const m = String(d.getMinutes()).padStart(2, "0");
  const period = h < 12 ? "上午" : "下午";
  const display = h === 0 ? 12 : h > 12 ? h - 12 : h;
  return `${period} ${String(display).padStart(2, "0")}:${m}`;
}

export function statusBadgeClass(status: MeetingStatus): string {
  switch (status) {
    case "draft":
      return "bg-[#FEF3C7] text-[#92400E] border-[#FCD34D]";
    case "scheduled":
      return "bg-[#DBEAFE] text-[#1E40AF] border-[#BFDBFE]";
    case "in_progress":
      return "bg-[#EDE9FE] text-[#5B21B6] border-[#DDD6FE]";
    case "completed":
      return "bg-[#D1FAE5] text-[#065F46] border-[#A7F3D0]";
    case "cancelled":
      return "bg-[#F1F5F9] text-[#64748B] border-[#E2E8F0]";
  }
}

/** Build an array of CalendarDay-keyed dates for the requested month. */
export function buildMonthGrid(year: number, month: number): Array<{
  date: Date;
  dayOfMonth: number;
  isCurrentMonth: boolean;
}> {
  const first = new Date(year, month - 1, 1);
  const startDow = first.getDay(); // 0=Sun
  const days: Array<{ date: Date; dayOfMonth: number; isCurrentMonth: boolean }> = [];

  // Leading days from previous month
  for (let i = startDow; i > 0; i--) {
    const d = new Date(year, month - 1, 1 - i);
    days.push({ date: d, dayOfMonth: d.getDate(), isCurrentMonth: false });
  }
  // Current month
  const lastOfMonth = new Date(year, month, 0).getDate();
  for (let i = 1; i <= lastOfMonth; i++) {
    const d = new Date(year, month - 1, i);
    days.push({ date: d, dayOfMonth: i, isCurrentMonth: true });
  }
  // Trailing to fill 6 weeks (42 cells)
  while (days.length < 42) {
    const last = days[days.length - 1].date;
    const d = new Date(last);
    d.setDate(last.getDate() + 1);
    days.push({ date: d, dayOfMonth: d.getDate(), isCurrentMonth: false });
  }
  return days;
}

export function isoDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function relativeFromNow(iso: string): string {
  const d = new Date(iso).getTime();
  const now = Date.now();
  const diff = Math.round((d - now) / 60000); // minutes
  if (diff > 0 && diff < 60) return `${diff} 分鐘後`;
  if (diff < 0 && diff > -60) return `${-diff} 分鐘前`;
  const hours = Math.round(diff / 60);
  if (hours > 0 && hours < 24) return `${hours} 小時後`;
  if (hours < 0 && hours > -24) return `${-hours} 小時前`;
  return formatDateTime(iso);
}
