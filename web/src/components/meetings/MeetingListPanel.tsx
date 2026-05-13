"use client";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { meetings as meetingsApi, type Meeting } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { formatTimeRange, isoDate, statusBadgeClass } from "./meeting-utils";

export function MeetingListPanel({ date }: { date: Date }) {
  const t = useT();
  const [items, setItems] = useState<Meeting[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    (async () => {
      try {
        const start = new Date(date);
        start.setHours(0, 0, 0, 0);
        const end = new Date(date);
        end.setHours(23, 59, 59, 999);
        const list = await meetingsApi.list({
          from: start.toISOString(),
          to: end.toISOString(),
        });
        if (!cancelled) setItems(list);
      } catch {
        if (!cancelled) setItems([]);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [date]);

  const isoSelected = useMemo(() => isoDate(date), [date]);
  const today = isoDate(new Date());
  const headerLabel = isoSelected === today ? "今天" : isoSelected;
  const summarizableCount = items.filter((m) => m.status === "completed" || m.status === "in_progress").length;

  return (
    <aside className="flex w-[360px] flex-shrink-0 flex-col gap-4 overflow-y-auto rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <div>
        <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
          {t("meetings.list.title")}
        </div>
        <div className="mt-1 text-[12px] leading-5 text-[#94A3B8]">{t("meetings.list.desc")}</div>
      </div>

      <div className="rounded-xl bg-[#ECFDF5] px-3 py-2 text-[12px] font-medium text-[#065F46]">
        ● {t("meetings.list.todayBanner")
          .replace("{total}", String(items.length))
          .replace("{ready}", String(summarizableCount))}
      </div>

      <div className="text-[11px] text-[#94A3B8]">{headerLabel}</div>

      <div className="flex-1 space-y-3">
        {loading ? (
          <div className="text-[12px] text-[#94A3B8]">Loading…</div>
        ) : items.length === 0 ? (
          <div className="rounded-lg border border-dashed border-[#E2E8F0] px-3 py-6 text-center text-[12px] text-[#94A3B8]">
            {t("meetings.list.empty")}
          </div>
        ) : (
          items.map((m) => (
            <Link
              key={m.id}
              href={`/meetings/${m.id}`}
              className="block rounded-xl border border-[#E2E8F0] bg-white p-3 transition hover:border-[#CBD5E1] hover:shadow-sm"
            >
              <div className="flex items-center justify-between gap-2">
                <span className="text-[12px] font-medium text-[#1A1A2E]">
                  {formatTimeRange(m.start_at, m.end_at)}
                </span>
                <span
                  className={cn(
                    "rounded-full border px-2 py-0.5 text-[10px] font-medium",
                    statusBadgeClass(m.status)
                  )}
                >
                  {t(`meetings.status.${m.status}`)}
                </span>
              </div>
              <div className="mt-1.5 text-[14px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
                {m.title}
              </div>
              {m.location && (
                <div className="mt-1 text-[11px] text-[#94A3B8]">{m.location}</div>
              )}
              {m.notification_note && (
                <div className="mt-1.5 line-clamp-2 text-[12px] leading-5 text-[#475569]">
                  {m.notification_note}
                </div>
              )}
            </Link>
          ))
        )}
      </div>

      <div className="rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] p-3">
        <div className="text-[12px] font-semibold text-[#1A1A2E]">
          {t("meetings.list.nextStep")}
        </div>
        <div className="mt-1 text-[12px] leading-5 text-[#64748B]">{t("meetings.list.nextStepBody")}</div>
      </div>
    </aside>
  );
}
