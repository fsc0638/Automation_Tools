"use client";
import { type MeetingNotes } from "@/lib/api";
import { useT } from "@/lib/i18n";

export function NotesSummary({
  meetingId: _meetingId,
  notes,
  onChange: _onChange,
}: {
  meetingId: string;
  notes: MeetingNotes | null;
  onChange: () => void;
}) {
  const t = useT();

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
