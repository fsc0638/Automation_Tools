"use client";
import { useEffect, useState } from "react";
import { meetings as meetingsApi, type MeetingNotesEdit } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { formatDateTime } from "./meeting-utils";

export function NotesHistory({ meetingId }: { meetingId: string }) {
  const t = useT();
  const [edits, setEdits] = useState<MeetingNotesEdit[]>([]);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await meetingsApi.notesHistory(meetingId);
        if (!cancelled) setEdits(list);
      } catch {
        if (!cancelled) setEdits([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [meetingId]);

  const latestVer = edits[0]?.version ?? null;
  const shown = expanded ? edits : edits.slice(0, 3);

  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <header className="flex items-center justify-between">
        <div>
          <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
            {t("meetings.notes.historyTitle")}
          </div>
          <div className="mt-1 text-[12px] text-[#94A3B8]">{t("meetings.notes.historyDesc")}</div>
        </div>
        {latestVer && (
          <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2 py-0.5 text-[11px] text-[#475569]">
            v{latestVer} 最新
          </span>
        )}
      </header>

      <div className="mt-4 text-[11px] font-medium uppercase tracking-[0.06em] text-[#94A3B8]">
        TODAY
      </div>

      <ul className="mt-2 space-y-3 border-l-2 border-[#E2E8F0] pl-3">
        {shown.length === 0 ? (
          <li className="text-[12px] text-[#94A3B8]">尚無修改紀錄</li>
        ) : (
          shown.map((e, i) => (
            <li key={e.id} className="relative">
              <span className={`absolute -left-[1.05rem] top-1.5 h-2 w-2 rounded-full ${i === 0 ? "bg-[#0050A0]" : "bg-[#CBD5E1]"}`} />
              <div className="text-[12px] font-medium text-[#1A1A2E]">
                {formatDateTime(e.created_at).slice(-5)} · {e.editor_name ?? "—"} {e.edit_summary || `更新 v${e.version}`}
              </div>
              {e.edit_summary && (
                <div className="mt-1 rounded-md bg-[#F8FAFC] p-2 text-[12px] leading-5 text-[#475569]">
                  <div className="font-semibold text-[#1A1A2E]">修訂摘要</div>
                  <div className="mt-0.5">{e.edit_summary}</div>
                </div>
              )}
            </li>
          ))
        )}
      </ul>

      {edits.length > 3 && !expanded && (
        <button
          type="button"
          onClick={() => setExpanded(true)}
          className="mt-3 text-[12px] text-[#0050A0] hover:underline"
        >
          {t("meetings.notes.viewAll").replace("{n}", String(edits.length))} →
        </button>
      )}
    </div>
  );
}
