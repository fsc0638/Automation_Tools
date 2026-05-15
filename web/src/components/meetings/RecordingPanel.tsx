"use client";
import { useEffect, useRef, useState } from "react";
import { FileText, MoreHorizontal, Pause, Play, Square } from "lucide-react";
import { meetings as meetingsApi, type MeetingFile } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { formatDateTime } from "./meeting-utils";

type RecordingState = "idle" | "recording" | "paused" | "stopped";

/**
 * Browser MediaRecorder wrapper. On stop, blob is uploaded as a recording
 * file. We only render the controls — file list is rendered alongside us.
 */
export function RecordingPanel({
  meetingId,
  files,
  onFilesChange,
  canEdit,
}: {
  meetingId: string;
  files: MeetingFile[];
  onFilesChange: () => void;
  /** False ⇒ non-participant; render only the readonly recording list. */
  canEdit: boolean;
}) {
  const t = useT();
  const [state, setState] = useState<RecordingState>("idle");
  const [elapsed, setElapsed] = useState(0);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState("");
  const recorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const tickerRef = useRef<number | null>(null);
  const startedAtRef = useRef<number>(0);

  useEffect(() => {
    return () => {
      if (tickerRef.current) window.clearInterval(tickerRef.current);
      recorderRef.current?.stop();
    };
  }, []);

  function elapsedHMS(): string {
    const h = Math.floor(elapsed / 3600);
    const m = Math.floor((elapsed % 3600) / 60);
    const s = elapsed % 60;
    return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }

  async function start() {
    setError("");
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const recorder = new MediaRecorder(stream);
      chunksRef.current = [];
      recorder.ondataavailable = (e) => {
        if (e.data.size > 0) chunksRef.current.push(e.data);
      };
      recorder.start(1000);
      recorderRef.current = recorder;
      startedAtRef.current = Date.now();
      setState("recording");
      setElapsed(0);
      tickerRef.current = window.setInterval(() => {
        setElapsed(Math.floor((Date.now() - startedAtRef.current) / 1000));
      }, 1000);
    } catch (e) {
      setError(e instanceof Error ? e.message : "無法開始錄音");
    }
  }

  function pause() {
    recorderRef.current?.pause();
    setState("paused");
    if (tickerRef.current) window.clearInterval(tickerRef.current);
  }

  function resume() {
    recorderRef.current?.resume();
    setState("recording");
    const baseElapsed = elapsed;
    const resumeAt = Date.now();
    tickerRef.current = window.setInterval(() => {
      setElapsed(baseElapsed + Math.floor((Date.now() - resumeAt) / 1000));
    }, 1000);
  }

  async function stop() {
    if (tickerRef.current) window.clearInterval(tickerRef.current);
    const recorder = recorderRef.current;
    if (!recorder) return;
    setState("stopped");
    setUploading(true);
    await new Promise<void>((resolve) => {
      recorder.onstop = () => resolve();
      recorder.stop();
    });
    const blob = new Blob(chunksRef.current, { type: "audio/webm" });
    const filename = `meeting-recording-${new Date().toISOString().slice(0, 16).replace(/[:T]/g, "")}.webm`;
    const file = new File([blob], filename, { type: "audio/webm" });
    try {
      await meetingsApi.uploadFile(meetingId, file, {
        category: "recording",
        durationSeconds: elapsed,
      });
      onFilesChange();
      // Reset for the next recording session.
      setState("idle");
      setElapsed(0);
      recorderRef.current = null;
      chunksRef.current = [];
    } catch (e) {
      setError(e instanceof Error ? e.message : "上傳失敗");
    } finally {
      setUploading(false);
    }
  }

  const statusKey =
    state === "recording" ? "meetings.recording.recording" :
    state === "paused"    ? "meetings.recording.paused" :
    state === "stopped"   ? "meetings.recording.stopped" :
                            "meetings.recording.notStarted";
  const recordings = files.filter((f) => f.file_category === "recording");

  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <header className="mb-3 flex items-start justify-between">
        <div>
          <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
            {t("meetings.recording.title")}
          </div>
          <div className="mt-1 text-[12px] text-[#94A3B8]">{t("meetings.recording.desc")}</div>
        </div>
        <span className="flex items-center gap-1.5 text-[11px] text-[#C8102E]">
          <span className={cn("h-2 w-2 rounded-full", state === "recording" ? "bg-[#C8102E] animate-pulse" : "bg-[#CBD5E1]")} />
          {t("meetings.recording.standby")}
        </span>
      </header>

      {canEdit && (
      <div className="rounded-xl border border-[#FCA5A5] bg-[#FEE2E2] px-4 py-3">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-[11px] text-[#991B1B]">{t("meetings.recording.statusLabel")}</div>
            <div className="mt-1 text-[16px] font-semibold tracking-[-0.01em] text-[#991B1B]">
              {t(statusKey)}
            </div>
          </div>
          <div className="text-[18px] font-mono font-semibold tracking-tight text-[#991B1B]">
            {elapsedHMS()}
          </div>
        </div>
        <div className="mt-2 text-[12px] leading-5 text-[#991B1B]/85">
          {t("meetings.recording.bodyHint")}
        </div>

        <div className="mt-3 flex gap-2">
          {state === "idle" || state === "stopped" ? (
            <button
              type="button"
              onClick={() => void start()}
              disabled={uploading}
              className="inline-flex items-center gap-1.5 rounded-xl bg-[#C8102E] px-4 py-2 text-[13px] font-medium text-white hover:bg-[#A50D26] disabled:opacity-60"
            >
              <Play size={13} /> {t("meetings.recording.startBtn")}
            </button>
          ) : state === "recording" ? (
            <>
              <button
                type="button"
                onClick={pause}
                className="inline-flex items-center gap-1.5 rounded-xl border border-[#E2E8F0] bg-white px-4 py-2 text-[13px] font-medium text-[#1A1A2E] hover:bg-[#F8FAFC]"
              >
                <Pause size={13} /> {t("meetings.recording.pauseBtn")}
              </button>
              <button
                type="button"
                onClick={() => void stop()}
                className="inline-flex items-center gap-1.5 rounded-xl bg-[#1A1A2E] px-4 py-2 text-[13px] font-medium text-white hover:bg-[#243149]"
              >
                <Square size={13} /> {t("meetings.recording.stopBtn")}
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                onClick={resume}
                className="inline-flex items-center gap-1.5 rounded-xl border border-[#E2E8F0] bg-white px-4 py-2 text-[13px] font-medium text-[#1A1A2E] hover:bg-[#F8FAFC]"
              >
                <Play size={13} /> {t("meetings.recording.resumeBtn")}
              </button>
              <button
                type="button"
                onClick={() => void stop()}
                className="inline-flex items-center gap-1.5 rounded-xl bg-[#1A1A2E] px-4 py-2 text-[13px] font-medium text-white hover:bg-[#243149]"
              >
                <Square size={13} /> {t("meetings.recording.stopBtn")}
              </button>
            </>
          )}
        </div>
      </div>
      )}

      {error && canEdit && (
        <div className="mt-3 rounded-md bg-[#FEE2E2] px-2 py-1.5 text-[12px] text-[#991B1B]">
          {error}
        </div>
      )}

      <div className="mt-4">
        <div className="mb-2 flex items-center justify-between">
          <div className="text-[13px] font-semibold text-[#1A1A2E]">{t("meetings.recording.files")}</div>
        </div>
        {recordings.length === 0 ? (
          <div className="rounded-lg border border-dashed border-[#E2E8F0] px-3 py-4 text-center text-[12px] text-[#94A3B8]">
            尚無錄音檔
          </div>
        ) : (
          <ul className="space-y-2">
            {recordings.map((f) => (
              <li key={f.id} className="flex items-center justify-between rounded-xl border border-[#E2E8F0] bg-white px-3 py-2">
                <div className="flex items-center gap-2 min-w-0">
                  <FileText size={14} className="flex-shrink-0 text-[#94A3B8]" />
                  <div className="min-w-0">
                    <div className="truncate text-[13px] font-medium text-[#1A1A2E]">{f.filename}</div>
                    <div className="mt-0.5 text-[11px] text-[#94A3B8]">
                      {f.duration_seconds ? `${Math.floor(f.duration_seconds / 60)}m${f.duration_seconds % 60}s · ` : ""}
                      {formatDateTime(f.created_at)}
                    </div>
                  </div>
                </div>
                {canEdit && (
                  <button className="rounded-md p-1 text-[#94A3B8] hover:bg-[#F1F5F9]">
                    <MoreHorizontal size={14} />
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
