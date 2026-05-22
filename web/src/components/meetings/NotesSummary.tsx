"use client";
import { useState } from "react";
import {
  meetings as meetingsApi,
  type MeetingNotes,
  type ReconcileProposal,
  type SyncDecisionInput,
} from "@/lib/api";
import { useT } from "@/lib/i18n";

// One row in the confirm modal — the AI proposal plus the user's
// (possibly overridden) decision.
type DecisionRow = {
  title: string;
  description: string;
  decision: "new" | "continue" | "skip";
  target_task_id?: string;
  target_task_title?: string;
  target_task_status?: string;
  reason?: string;
};

function proposalToRow(p: ReconcileProposal): DecisionRow {
  // AI "duplicate" → default the user choice to skip (don't recreate);
  // they can still flip it. "continue"/"new" map straight through.
  const decision: DecisionRow["decision"] =
    p.suggested === "duplicate" ? "skip" : p.suggested;
  return {
    title: p.title,
    description: p.description,
    decision,
    target_task_id: p.target_task_id,
    target_task_title: p.target_task_title,
    target_task_status: p.target_task_status,
    reason: p.reason,
  };
}

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
  // Two-step state: null = closed; array = preview loaded, modal open.
  const [rows, setRows] = useState<DecisionRow[] | null>(null);

  async function openPreview() {
    if (syncing) return;
    setSyncing(true);
    setSyncMsg("");
    try {
      const r = await meetingsApi.syncTasksPreview(meetingId);
      if (r.proposals.length === 0) {
        setSyncMsg("沒有可同步的待辦事項。");
      } else {
        setRows(r.proposals.map(proposalToRow));
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : "比對失敗";
      setSyncMsg(
        /project/i.test(msg)
          ? "此會議未連結至專案，無法同步成任務。"
          : /lock/i.test(msg)
            ? "會議已鎖定，請先重新開啟。"
            : msg
      );
    } finally {
      setSyncing(false);
    }
  }

  function setRowDecision(i: number, decision: DecisionRow["decision"]) {
    setRows((prev) =>
      prev ? prev.map((r, idx) => (idx === i ? { ...r, decision } : r)) : prev
    );
  }

  async function confirmSync() {
    if (!rows || syncing) return;
    setSyncing(true);
    try {
      const decisions: SyncDecisionInput[] = rows.map((r) => ({
        title: r.title,
        decision: r.decision,
        target_task_id:
          r.decision === "continue" ? r.target_task_id : undefined,
      }));
      const res = await meetingsApi.syncNotesToTasks(meetingId, decisions);
      const c = res.created_task_ids.length;
      const l = res.linked_task_ids.length;
      const s = res.skipped_existing_titles.length;
      setSyncMsg(
        `已建立 ${c} 筆新任務、連結 ${l} 筆既有任務${s > 0 ? `、略過 ${s} 筆` : ""}。`
      );
      setRows(null);
      onChange();
    } catch (e) {
      setSyncMsg(e instanceof Error ? e.message : "同步失敗");
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
      {/* History-aware reconcile confirm modal */}
      {rows && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
          <div className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded-2xl border border-[#E2E8F0] bg-white shadow-xl">
            <div className="border-b border-[#E2E8F0] px-5 py-4">
              <div className="text-[16px] font-semibold text-[#1A1A2E]">
                確認同步（已比對專案歷史）
              </div>
              <div className="mt-1 text-[12px] text-[#94A3B8]">
                AI 已比對專案現有任務。延續既有的不會重建，只連結並在舊任務留跟進註記。
              </div>
            </div>
            <div className="flex-1 space-y-3 overflow-auto px-5 py-4">
              {rows.map((r, i) => (
                <div
                  key={i}
                  className="rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] p-3"
                >
                  <div className="text-[13px] font-medium text-[#1A1A2E]">
                    {r.title}
                  </div>
                  {r.reason && (
                    <div className="mt-0.5 text-[11px] text-[#64748B]">
                      AI 判斷：{r.reason}
                    </div>
                  )}
                  {r.target_task_title && (
                    <div className="mt-1 text-[11px] text-[#475569]">
                      對應既有任務：
                      <span className="font-medium">{r.target_task_title}</span>
                      {r.target_task_status && (
                        <span className="ml-1 text-[#94A3B8]">
                          （{r.target_task_status}）
                        </span>
                      )}
                    </div>
                  )}
                  <div className="mt-2 flex gap-1.5">
                    {([
                      ["new", "建新任務"],
                      ["continue", "連結既有 + 跟進註記"],
                      ["skip", "略過"],
                    ] as Array<[DecisionRow["decision"], string]>).map(
                      ([val, label]) => {
                        const disabled =
                          val === "continue" && !r.target_task_id;
                        return (
                          <button
                            key={val}
                            type="button"
                            disabled={disabled}
                            onClick={() => setRowDecision(i, val)}
                            className={
                              "rounded-md border px-2 py-1 text-[11px] font-medium transition " +
                              (r.decision === val
                                ? "border-[#0050A0] bg-[#EFF6FF] text-[#0050A0]"
                                : disabled
                                  ? "border-[#E2E8F0] bg-white text-[#CBD5E1] cursor-not-allowed"
                                  : "border-[#E2E8F0] bg-white text-[#475569] hover:bg-[#F1F5F9]")
                            }
                          >
                            {label}
                          </button>
                        );
                      }
                    )}
                  </div>
                </div>
              ))}
            </div>
            <div className="flex justify-end gap-2 border-t border-[#E2E8F0] px-5 py-3">
              <button
                type="button"
                onClick={() => setRows(null)}
                disabled={syncing}
                className="rounded-xl border border-[#E2E8F0] bg-white px-3.5 py-2 text-[13px] font-medium text-[#475569] hover:bg-[#F8FAFC] disabled:opacity-50"
              >
                取消
              </button>
              <button
                type="button"
                onClick={() => void confirmSync()}
                disabled={syncing}
                className="rounded-xl bg-[#1A1A2E] px-3.5 py-2 text-[13px] font-medium text-white hover:bg-[#243149] disabled:opacity-50"
              >
                {syncing ? "同步中…" : "確認同步"}
              </button>
            </div>
          </div>
        </div>
      )}

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
                onClick={() => void openPreview()}
                disabled={syncing}
                className="rounded-md border border-[#0050A0] bg-white px-2.5 py-1 text-[11px] font-medium text-[#0050A0] hover:bg-[#EFF6FF] disabled:opacity-50"
              >
                {syncing && !rows ? "比對中…" : "同步成任務"}
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
