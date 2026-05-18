"use client";
/**
 * Project meeting continuity timeline. Same component drives two views:
 *  - detail page: pass `limit={3}` for a condensed "本專案會議歷史" panel
 *  - project page 會議 tab: no limit, full history
 *
 * Each meeting row shows summary + decisions + action items, and every
 * action item carries the LIVE status of the task it became (matched
 * casefold by title on the backend). This is the "可追蹤 + 知道歷史 +
 * 持續演化跟進" surface.
 */
import { useEffect, useState } from "react";
import Link from "next/link";
import {
  meetings as meetingsApi,
  type ProjectMeetingHistoryItem,
} from "@/lib/api";

function statusBadge(status: string): { label: string; cls: string } {
  switch (status) {
    case "done":
      return { label: "已完成", cls: "bg-[#D1FAE5] text-[#065F46]" };
    case "in_progress":
    case "doing":
      return { label: "進行中", cls: "bg-[#DBEAFE] text-[#1E40AF]" };
    case "todo":
      return { label: "待辦", cls: "bg-[#FEF3C7] text-[#92400E]" };
    default:
      return { label: status, cls: "bg-[#F1F5F9] text-[#475569]" };
  }
}

function fmtDate(iso: string): string {
  const d = new Date(iso);
  return `${d.getFullYear()}/${String(d.getMonth() + 1).padStart(2, "0")}/${String(d.getDate()).padStart(2, "0")}`;
}

export function ProjectMeetingTimeline({
  projectId,
  limit,
  currentMeetingId,
  title = "本專案會議歷史",
}: {
  projectId: string;
  /** Cap rows (detail page uses 3). Undefined ⇒ show all. */
  limit?: number;
  /** Highlight / skip the row for the meeting being viewed. */
  currentMeetingId?: string;
  title?: string;
}) {
  const [items, setItems] = useState<ProjectMeetingHistoryItem[] | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await meetingsApi.projectMeetingHistory(projectId);
        if (!cancelled) setItems(list);
      } catch (e) {
        if (!cancelled)
          setError(e instanceof Error ? e.message : "讀取會議歷史失敗");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [projectId]);

  if (error) {
    return (
      <div className="rounded-2xl border border-[#FCA5A5] bg-[#FEE2E2] p-4 text-[13px] text-[#991B1B]">
        {error}
      </div>
    );
  }
  if (items === null) {
    return (
      <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5 text-[13px] text-[#94A3B8]">
        載入會議歷史中…
      </div>
    );
  }

  const shown = limit ? items.slice(0, limit) : items;

  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <div className="mb-1 flex items-center justify-between">
        <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
          {title}
        </div>
        <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2 py-0.5 text-[11px] text-[#475569]">
          共 {items.length} 場
        </span>
      </div>
      <div className="mb-4 text-[12px] text-[#94A3B8]">
        決議與待辦持續演化，待辦右側顯示任務當下狀態
      </div>

      {shown.length === 0 ? (
        <div className="rounded-lg border border-dashed border-[#E2E8F0] px-3 py-6 text-center text-[12px] text-[#94A3B8]">
          這個專案還沒有任何會議
        </div>
      ) : (
        <ol className="relative space-y-4 border-l border-[#E2E8F0] pl-4">
          {shown.map((m) => {
            const isCurrent = m.meeting_id === currentMeetingId;
            return (
              <li key={m.meeting_id} className="relative">
                <span
                  className={
                    "absolute -left-[21px] top-1 h-2.5 w-2.5 rounded-full " +
                    (isCurrent ? "bg-[#0050A0]" : "bg-[#CBD5E1]")
                  }
                />
                <div
                  className={
                    "rounded-xl border p-3 " +
                    (isCurrent
                      ? "border-[#0050A0] bg-[#EFF6FF]"
                      : "border-[#E2E8F0] bg-white")
                  }
                >
                  <div className="flex items-center justify-between gap-2">
                    <Link
                      href={`/meetings/${m.meeting_id}`}
                      className="truncate text-[13px] font-semibold text-[#1A1A2E] hover:underline"
                    >
                      {m.title}
                    </Link>
                    <span className="shrink-0 text-[11px] text-[#94A3B8]">
                      {fmtDate(m.start_at)}
                      {isCurrent && (
                        <span className="ml-1 text-[#0050A0]">· 本場</span>
                      )}
                    </span>
                  </div>

                  {m.summary && (
                    <div className="mt-1.5 text-[12px] leading-5 text-[#475569]">
                      {m.summary}
                    </div>
                  )}

                  {m.decisions.length > 0 && (
                    <div className="mt-2">
                      <div className="text-[11px] font-semibold text-[#1A1A2E]">
                        決議
                      </div>
                      <ul className="mt-1 space-y-0.5 text-[12px] text-[#1A1A2E]">
                        {m.decisions.map((d, i) => (
                          <li key={i} className="flex gap-1.5">
                            <span className="text-[#10B981]">•</span>
                            <span>{d.text}</span>
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}

                  {m.action_items.length > 0 && (
                    <div className="mt-2">
                      <div className="text-[11px] font-semibold text-[#1A1A2E]">
                        待辦 / 跟進
                      </div>
                      <ul className="mt-1 space-y-1">
                        {m.action_items.map((a, i) => {
                          const badge = a.task_status
                            ? statusBadge(a.task_status)
                            : null;
                          return (
                            <li
                              key={i}
                              className="flex items-start justify-between gap-2 text-[12px]"
                            >
                              <span className="flex-1 text-[#1A1A2E]">
                                <span className="text-[#0050A0]">▸</span>{" "}
                                {a.title}
                                {a.assignee_name && (
                                  <span className="ml-1 text-[11px] text-[#94A3B8]">
                                    @{a.assignee_name}
                                  </span>
                                )}
                              </span>
                              {badge ? (
                                <span
                                  className={
                                    "shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium " +
                                    badge.cls
                                  }
                                >
                                  {badge.label}
                                </span>
                              ) : (
                                <span className="shrink-0 rounded-full bg-[#F1F5F9] px-2 py-0.5 text-[10px] text-[#94A3B8]">
                                  未建任務
                                </span>
                              )}
                            </li>
                          );
                        })}
                      </ul>
                    </div>
                  )}
                </div>
              </li>
            );
          })}
        </ol>
      )}

      {limit && items.length > limit && (
        <div className="mt-3 text-center text-[12px] text-[#64748B]">
          僅顯示最近 {limit} 場；完整時間軸見專案頁「會議」分頁
        </div>
      )}
    </div>
  );
}
