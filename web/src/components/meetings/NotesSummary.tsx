"use client";
import { useState } from "react";
import { meetings as meetingsApi, type MeetingNotes } from "@/lib/api";
import { useT } from "@/lib/i18n";

export function NotesSummary({
  meetingId,
  notes,
  onChange,
  canEdit,
}: {
  meetingId: string;
  notes: MeetingNotes | null;
  onChange: () => void;
  /** False ⇒ hide 同步成任務 button. Read of summary/decisions/risks
   *  remains visible. */
  canEdit: boolean;
}) {
  const t = useT();
  const [syncing, setSyncing] = useState(false);
  const [syncMsg, setSyncMsg] = useState<string>("");

  async function handleSyncTasks() {
    if (syncing) return;
    setSyncing(true);
    setSyncMsg("");
    try {
      const r = await meetingsApi.syncNotesToTasks(meetingId);
      const created = r.created_task_ids.length;
      const skipped = r.skipped_existing_titles.length;
      setSyncMsg(
        created === 0 && skipped === 0
          ? "沒有可同步的待辦事項。"
          : `已建立 ${created} 筆任務${skipped > 0 ? `；略過 ${skipped} 筆同名` : ""}。`
      );
      onChange();
    } catch (e) {
      const msg = e instanceof Error ? e.message : "同步失敗";
      // 400 when the meeting has no project_id is the common path.
      setSyncMsg(/project/i.test(msg) ? "此會議未連結至專案，無法同步成任務。" : msg);
    } finally {
      setSyncing(false);
    }
  }

  if (!notes) {
    return (
      <div className="rounded-2xl border border-dashed border-[#E2E8F0] bg-white px-5 py-10 text-center text-[13px] text-[#94A3B8]">
        尚未產生會議紀錄。請於「會議資訊」頁面點擊「產出會議紀錄」。
      </div>
    );
  }

  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <div className="flex items-center justify-between">
        <div className="text-[18px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">會議紀錄</div>
        <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2 py-0.5 text-[11px] text-[#475569]">
          {t("meetings.notes.editable")}
        </span>
      </div>

      <section className="mt-5">
        <div className="text-[13px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
          {t("meetings.notes.summary")}
        </div>
        <div className="mt-2 rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] p-3 text-[13px] leading-6 text-[#1A1A2E]">
          {notes.summary || "（未產生摘要）"}
        </div>
      </section>

      {(notes.decisions.length > 0 || notes.risks.length > 0) && (
        <section className="mt-5">
          <div className="text-[13px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
            {t("meetings.notes.decisions")} / {t("meetings.notes.risks")}
          </div>
          <div className="mt-2 grid gap-4 md:grid-cols-2">
            <div className="rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] p-3">
              <div className="text-[12px] font-semibold text-[#1A1A2E]">已確認的決策：</div>
              <ul className="mt-1.5 space-y-1 text-[13px] leading-6 text-[#1A1A2E]">
                {notes.decisions.map((d, i) => (
                  <li key={i} className="flex gap-1.5">
                    <span className="text-[#10B981]">•</span>
                    <span>{d.text}</span>
                  </li>
                ))}
                {notes.decisions.length === 0 && (
                  <li className="text-[#94A3B8]">尚無</li>
                )}
              </ul>
            </div>
            <div className="rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] p-3">
              <div className="text-[12px] font-semibold text-[#1A1A2E]">待追蹤風險：</div>
              <ul className="mt-1.5 space-y-1 text-[13px] leading-6 text-[#1A1A2E]">
                {notes.risks.map((r, i) => (
                  <li key={i} className="flex gap-1.5">
                    <span
                      className={
                        r.severity === "high"
                          ? "text-[#C8102E]"
                          : r.severity === "medium"
                            ? "text-[#F59E0B]"
                            : "text-[#94A3B8]"
                      }
                    >
                      •
                    </span>
                    <span>{r.text}</span>
                  </li>
                ))}
                {notes.risks.length === 0 && (
                  <li className="text-[#94A3B8]">尚無</li>
                )}
              </ul>
            </div>
          </div>
        </section>
      )}

      {/* AgentK-aligned action items (migration 0033). Visually mirrors
       *  decisions/risks so the demo lines up with AgentK.pen's records
       *  layout. The "同步成任務" sync-to-tasks endpoint is deferred —
       *  this panel is read-only for now and just renders whatever the
       *  notes carry. */}
      {notes.action_items && notes.action_items.length > 0 && (
        <section className="mt-5">
          <div className="flex items-center justify-between">
            <div className="text-[13px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
              待辦事項（Action Items）
            </div>
            {canEdit && (
              <button
                type="button"
                onClick={() => void handleSyncTasks()}
                disabled={syncing}
                className="rounded-md border border-[#0050A0] bg-white px-2.5 py-1 text-[11px] font-medium text-[#0050A0] hover:bg-[#EFF6FF] disabled:opacity-50"
              >
                {syncing ? "同步中…" : "同步成任務"}
              </button>
            )}
          </div>
          {syncMsg && (
            <div className="mt-1 text-[11px] text-[#64748B]">{syncMsg}</div>
          )}
          <div className="mt-2 space-y-2">
            {notes.action_items.map((item, i) => (
              <div key={i} className="rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] p-3">
                <div className="flex items-start gap-2 text-[13px] text-[#1A1A2E]">
                  <span className="mt-0.5 text-[#0050A0]">▸</span>
                  <div className="flex-1">
                    <div className="font-medium">{item.title}</div>
                    {item.description && (
                      <div className="mt-0.5 text-[12px] leading-5 text-[#475569]">
                        {item.description}
                      </div>
                    )}
                    {(item.assignee_user_id || item.assignee_name || item.source) && (
                      <div className="mt-1 flex flex-wrap gap-2 text-[11px] text-[#94A3B8]">
                        {(item.assignee_name || item.assignee_user_id) && (
                          <span>
                            負責人：{item.assignee_name ?? `#${item.assignee_user_id!.slice(0, 8)}`}
                            {item.assignee_user_id && item.assignee_name && (
                              <span className="ml-1 text-[#10B981]">●已對應帳號</span>
                            )}
                          </span>
                        )}
                        {item.source && <span>來源：{item.source}</span>}
                      </div>
                    )}
                  </div>
                </div>
              </div>
            ))}
          </div>
          {notes.task_ids && notes.task_ids.length > 0 && (
            <div className="mt-2 text-[11px] text-[#64748B]">
              已同步為 {notes.task_ids.length} 筆任務
            </div>
          )}
        </section>
      )}

      {notes.transcript_excerpts.length > 0 && (
        <section className="mt-5">
          <div className="text-[13px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
            {t("meetings.notes.transcript")}
          </div>
          <div className="mt-2 space-y-3">
            {notes.transcript_excerpts.map((ex, i) => (
              <div key={i} className="rounded-xl border border-[#E2E8F0] bg-white p-3">
                <div className="text-[12px] font-semibold text-[#1A1A2E]">
                  {ex.time} · {ex.speaker}
                </div>
                <div className="mt-1 text-[13px] leading-6 text-[#475569]">{ex.content}</div>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
