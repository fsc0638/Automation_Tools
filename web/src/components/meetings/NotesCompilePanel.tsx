"use client";
import { useState } from "react";
import { FileSearch, Sparkles } from "lucide-react";
import { meetings as meetingsApi, type MeetingFile, type MeetingNotes } from "@/lib/api";
import { useT } from "@/lib/i18n";

export function NotesCompilePanel({
  meetingId,
  files,
  latestNotes,
  onChange,
  canEdit,
}: {
  meetingId: string;
  files: MeetingFile[];
  latestNotes: MeetingNotes | null;
  onChange: () => void;
  /** False ⇒ hide AI generate button; only the file-count summary stays. */
  canEdit: boolean;
}) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [previewing, setPreviewing] = useState(false);

  const textAttachments = files.filter(
    (f) => f.file_category === "transcript" ||
      (f.file_category === "attachment" &&
        (f.mime_type === "text/plain" ||
          f.mime_type === "text/markdown" ||
          /\.(txt|md|json)$/i.test(f.filename)))
  );

  async function handleGenerate() {
    setError("");
    setBusy(true);
    try {
      await meetingsApi.generateNotes(meetingId);
      onChange();
    } catch (e) {
      setError(e instanceof Error ? e.message : "產出失敗");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <header className="mb-3 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Sparkles size={16} className="text-[#7C3AED]" />
          <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
            {t("meetings.notes.compileTitle")}
          </div>
        </div>
        <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2 py-0.5 text-[11px] text-[#475569]">
          Owner
        </span>
      </header>

      <p className="text-[13px] leading-6 text-[#475569]">
        {t("meetings.notes.compileBody")}
      </p>

      <div className="mt-3 rounded-lg bg-[#F8FAFC] px-3 py-2 text-[12px] text-[#475569]">
        可用文字附件：<strong className="text-[#1A1A2E]">{textAttachments.length}</strong> 份
        {latestNotes && (
          <span className="ml-3">
            最新版本：<strong className="text-[#1A1A2E]">v{latestNotes.version}</strong>
          </span>
        )}
      </div>

      {error && (
        <div className="mt-3 rounded-md bg-[#FEE2E2] px-2 py-1.5 text-[12px] text-[#991B1B]">
          {error}
        </div>
      )}

      <div className="mt-4 flex gap-2">
        {canEdit && (
          <button
            type="button"
            onClick={() => void handleGenerate()}
            disabled={busy || textAttachments.length === 0}
            className="inline-flex items-center gap-1.5 rounded-xl bg-[#1A1A2E] px-3.5 py-2 text-[13px] font-medium text-white hover:bg-[#243149] disabled:opacity-60"
          >
            {busy ? "產出中…" : t("meetings.notes.generate")}
          </button>
        )}
        <button
          type="button"
          onClick={() => setPreviewing((v) => !v)}
          disabled={!latestNotes}
          className="inline-flex items-center gap-1.5 rounded-xl border border-[#E2E8F0] bg-white px-3.5 py-2 text-[13px] font-medium text-[#475569] hover:bg-[#F8FAFC] disabled:opacity-60"
        >
          <FileSearch size={13} /> {t("meetings.notes.preview")}
        </button>
      </div>

      {previewing && latestNotes && (
        <div className="mt-4 rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] p-3 text-[13px] leading-6 text-[#1A1A2E]">
          <div className="font-semibold text-[#1A1A2E]">摘要預覽（v{latestNotes.version}）</div>
          <div className="mt-1.5 whitespace-pre-line text-[#475569]">
            {latestNotes.summary || "（未產生）"}
          </div>
        </div>
      )}
    </div>
  );
}
