"use client";
import { useRef, useState } from "react";
import { FileText, MoreHorizontal, Upload } from "lucide-react";
import { meetings as meetingsApi, type MeetingFile } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { formatDateTime } from "./meeting-utils";

type Tab = "merge" | "upload";

export function FileWorkspace({
  meetingId,
  files,
  uploaderNames,
  onChange,
}: {
  meetingId: string;
  files: MeetingFile[];
  uploaderNames: Map<string, string>;
  onChange: () => void;
}) {
  const t = useT();
  const [tab, setTab] = useState<Tab>("merge");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  const attachments = files.filter((f) => f.file_category === "attachment");

  async function handleUpload(list: FileList) {
    setError("");
    setBusy(true);
    try {
      for (const file of Array.from(list)) {
        await meetingsApi.uploadFile(meetingId, file, { category: "attachment" });
      }
      onChange();
    } catch (e) {
      setError(e instanceof Error ? e.message : "上傳失敗");
    } finally {
      setBusy(false);
      if (inputRef.current) inputRef.current.value = "";
    }
  }

  async function handleDelete(fileId: string) {
    setBusy(true);
    try {
      await meetingsApi.deleteFile(meetingId, fileId);
      onChange();
    } catch (e) {
      setError(e instanceof Error ? e.message : "刪除失敗");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <header className="mb-3 flex items-start justify-between">
        <div>
          <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
            {t("meetings.files.workspaceTitle")}
          </div>
          <div className="mt-1 text-[12px] text-[#94A3B8]">{t("meetings.files.workspaceDesc")}</div>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => setTab("merge")}
            className={cn(
              "rounded-xl border px-3 py-1.5 text-[12px] font-medium transition",
              tab === "merge"
                ? "border-[#1A1A2E] bg-[#1A1A2E] text-white"
                : "border-[#E2E8F0] bg-white text-[#475569] hover:bg-[#F8FAFC]"
            )}
          >
            {t("meetings.files.mergeBtn")}
          </button>
          <button
            type="button"
            onClick={() => {
              setTab("upload");
              inputRef.current?.click();
            }}
            className={cn(
              "rounded-xl px-3 py-1.5 text-[12px] font-medium transition",
              "bg-[#1A1A2E] text-white hover:bg-[#243149]"
            )}
          >
            {t("meetings.files.uploadBtn")}
          </button>
          <input
            ref={inputRef}
            type="file"
            multiple
            className="hidden"
            onChange={(e) => {
              if (e.target.files && e.target.files.length > 0) {
                void handleUpload(e.target.files);
              }
            }}
          />
        </div>
      </header>

      {error && (
        <div className="mb-3 rounded-md bg-[#FEE2E2] px-2 py-1.5 text-[12px] text-[#991B1B]">{error}</div>
      )}

      {attachments.length === 0 ? (
        <label className="flex cursor-pointer flex-col items-center justify-center rounded-xl border-2 border-dashed border-[#CBD5E1] bg-[#F8FAFC] py-10">
          <Upload size={20} className="text-[#94A3B8]" />
          <span className="mt-2 text-[13px] font-medium text-[#1A1A2E]">
            {t("meetings.attachments.upload")}
          </span>
          <input
            type="file"
            multiple
            className="hidden"
            onChange={(e) => {
              if (e.target.files && e.target.files.length > 0) {
                void handleUpload(e.target.files);
              }
            }}
          />
        </label>
      ) : (
        <ul className="space-y-2">
          {attachments.map((f) => (
            <li key={f.id} className="flex items-center justify-between rounded-xl border border-[#E2E8F0] bg-white px-3 py-2.5">
              <div className="flex items-center gap-2 min-w-0">
                <FileText size={14} className="flex-shrink-0 text-[#94A3B8]" />
                <div className="min-w-0">
                  <div className="truncate text-[13px] font-medium text-[#1A1A2E]">{f.filename}</div>
                  <div className="mt-0.5 text-[11px] text-[#94A3B8]">
                    {uploaderNames.get(f.uploader_id) ? `由 ${uploaderNames.get(f.uploader_id)} 上傳 · ` : ""}
                    {formatDateTime(f.created_at)}
                  </div>
                </div>
              </div>
              <button
                onClick={() => void handleDelete(f.id)}
                disabled={busy}
                className="rounded-md p-1 text-[#94A3B8] hover:bg-[#F1F5F9] disabled:opacity-50"
              >
                <MoreHorizontal size={14} />
              </button>
            </li>
          ))}
        </ul>
      )}

      <div className="mt-4 rounded-xl border border-[#DBEAFE] bg-[#EFF6FF] px-3 py-2.5 text-[12px] leading-5 text-[#1E40AF]">
        <div className="font-semibold">使用方式</div>
        <div className="mt-0.5">{t("meetings.files.ownerHint")}</div>
      </div>
    </div>
  );
}
