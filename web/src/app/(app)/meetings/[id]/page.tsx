"use client";
import { use, useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { ArrowLeft, Trash2 } from "lucide-react";
import { MeetingSidebar } from "@/components/meetings/MeetingSidebar";
import { RecordingPanel } from "@/components/meetings/RecordingPanel";
import { FileWorkspace } from "@/components/meetings/FileWorkspace";
import { NotesSummary } from "@/components/meetings/NotesSummary";
import { NotesCompilePanel } from "@/components/meetings/NotesCompilePanel";
import { AttendeeSignoff } from "@/components/meetings/AttendeeSignoff";
import { TaskImpactList } from "@/components/meetings/TaskImpactList";
import { NotesHistory } from "@/components/meetings/NotesHistory";
import { meetings as meetingsApi, type MeetingDetail } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { formatDateOnly, formatTimeRange } from "@/components/meetings/meeting-utils";

type Tab = "info" | "record";

export default function MeetingViewPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const t = useT();
  const router = useRouter();
  const { id } = use(params);
  const [tab, setTab] = useState<Tab>("info");
  const [detail, setDetail] = useState<MeetingDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    try {
      const d = await meetingsApi.get(id);
      setDetail(d);
    } catch (e) {
      setError(e instanceof Error ? e.message : "讀取失敗");
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (loading) {
    return (
      <div className="flex h-screen">
        <MeetingSidebar activeMeetingId={id} />
        <div className="flex flex-1 items-center justify-center text-[#94A3B8]">Loading…</div>
      </div>
    );
  }
  if (error || !detail) {
    return (
      <div className="flex h-screen">
        <MeetingSidebar activeMeetingId={id} />
        <div className="flex flex-1 items-center justify-center text-[#C8102E]">{error || "Not found"}</div>
      </div>
    );
  }

  const isDraft = detail.status === "draft";

  async function handleCompleteReady() {
    try {
      await meetingsApi.sendInvitations(id);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "操作失敗");
    }
  }

  async function handleDelete() {
    // Native confirm keeps the flow honest — meeting delete cascades to
    // attendees / files / notes on the DB side and removes on-disk files
    // on the backend, so there's no recovery once it goes through.
    if (!window.confirm(`確定要刪除「${detail!.title}」？\n\n此動作會一併刪除與會人、檔案、會議記錄，且無法復原。`)) {
      return;
    }
    try {
      await meetingsApi.delete(id);
      router.push("/meetings");
    } catch (e) {
      const msg = e instanceof Error ? e.message : "刪除失敗";
      // Backend returns 403 when caller isn't creator/project admin; surface
      // that plainly rather than as a generic failure.
      setError(/forbidden|403/i.test(msg) ? "您沒有刪除此會議的權限（僅會議建立者或所屬專案的 Owner / Admin 可刪除）" : msg);
    }
  }

  async function handleGenerate() {
    // Placeholder for P5 — wired to /notes/generate when the AI route exists.
    // For now just open the notes tab so the user can see existing notes.
    setTab("record");
  }

  const uploaderNames = new Map<string, string>();
  for (const a of detail.attendees) {
    if (a.user_id) uploaderNames.set(a.user_id, a.display_name || a.email);
  }

  return (
    <div className="flex h-screen overflow-hidden">
      <MeetingSidebar activeMeetingId={id} />

      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <header className="flex items-start justify-between border-b border-[#E2E8F0] bg-white px-6 py-4">
          <div className="flex items-start gap-3">
            <button
              type="button"
              onClick={() => router.back()}
              aria-label="返回上一頁"
              className="mt-0.5 inline-flex h-8 w-8 items-center justify-center rounded-lg border border-[#E2E8F0] bg-white text-[#475569] hover:bg-[#F8FAFC] hover:text-[#1A1A2E]"
            >
              <ArrowLeft size={16} />
            </button>
            <div>
              <h1 className="text-[20px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
                {t("meetings.viewTitle")}
              </h1>
              <p className="mt-1 text-[12px] text-[#94A3B8]">{t("meetings.viewDesc")}</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => void handleDelete()}
              title="刪除會議"
              aria-label="刪除會議"
              className="inline-flex h-9 w-9 items-center justify-center rounded-xl border border-[#FCA5A5] bg-white text-[#C8102E] transition hover:bg-[#FEE2E2]"
            >
              <Trash2 size={16} />
            </button>
            <button
              type="button"
              onClick={() => router.push(`/meetings/new?clone=${id}`)}
              className="rounded-xl border border-[#E2E8F0] bg-white px-3.5 py-2 text-[13px] font-medium text-[#1A1A2E] hover:bg-[#F8FAFC]"
            >
              {isDraft ? t("meetings.action.editInfo") : t("meetings.action.exportRecord")}
            </button>
            <button
              type="button"
              onClick={() => void (isDraft ? handleCompleteReady() : handleGenerate())}
              className="rounded-xl bg-[#1A1A2E] px-3.5 py-2 text-[13px] font-medium text-white hover:bg-[#243149]"
            >
              {isDraft ? t("meetings.action.completeReady") : t("meetings.action.shareRecord")}
            </button>
          </div>
        </header>

        {error && (
          <div className="flex items-center justify-between border-b border-[#FCA5A5] bg-[#FEE2E2] px-6 py-2 text-[13px] text-[#991B1B]">
            <span>{error}</span>
            <button
              type="button"
              onClick={() => setError("")}
              className="text-[11px] text-[#991B1B] underline"
            >
              關閉
            </button>
          </div>
        )}

        {/* Tab strip */}
        <div className="flex gap-2 border-b border-[#E2E8F0] bg-white px-6">
          {(["info", "record"] as const).map((k) => (
            <button
              key={k}
              type="button"
              onClick={() => setTab(k)}
              className={cn(
                "border-b-2 px-3 py-3 text-[13px] font-medium transition",
                tab === k
                  ? "border-[#1A1A2E] text-[#1A1A2E]"
                  : "border-transparent text-[#94A3B8] hover:text-[#1A1A2E]"
              )}
            >
              {t(`meetings.tab.${k}`)}
            </button>
          ))}
        </div>

        <div className="flex min-h-0 flex-1 gap-5 overflow-auto p-6">
          {tab === "info" ? (
            <InfoTab detail={detail} uploaderNames={uploaderNames} onChange={refresh} />
          ) : (
            <RecordTab detail={detail} onChange={refresh} />
          )}
        </div>
      </div>
    </div>
  );
}

function InfoTab({
  detail,
  uploaderNames,
  onChange,
}: {
  detail: MeetingDetail;
  uploaderNames: Map<string, string>;
  onChange: () => void;
}) {
  const t = useT();

  return (
    <>
      <section className="flex-1 space-y-5">
        <ReadonlyInfoCard detail={detail} />
        <NotesCompilePanel
          meetingId={detail.id}
          files={detail.files}
          latestNotes={detail.latest_notes}
          onChange={onChange}
        />
      </section>

      <aside className="flex w-[400px] flex-shrink-0 flex-col gap-5 overflow-y-auto">
        <RecordingPanel meetingId={detail.id} files={detail.files} onFilesChange={onChange} />
        <FileWorkspace
          meetingId={detail.id}
          files={detail.files}
          uploaderNames={uploaderNames}
          onChange={onChange}
        />
      </aside>
    </>
  );
}

function ReadonlyInfoCard({ detail }: { detail: MeetingDetail }) {
  const t = useT();
  const dateOnly = formatDateOnly(detail.start_at);
  const endDateOnly = formatDateOnly(detail.end_at);
  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <div className="mb-1 text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
        {t("meetings.basicInfo")}
      </div>
      <div className="mb-4 text-[12px] text-[#94A3B8]">{t("meetings.readonly")}</div>

      <div className="grid grid-cols-3 gap-4">
        <Field label={t("meetings.field.name")} value={detail.title} span={2} />
        <Field label={t("meetings.field.importance")} value={t(`meetings.importance.${detail.importance}`)} />
        <Field label={t("meetings.field.startDate")} value={dateOnly} />
        <Field
          label={t("meetings.field.startTime")}
          value={new Date(detail.start_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false })}
        />
        <Field
          label={t("meetings.field.endTime")}
          value={new Date(detail.end_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false })}
        />
        <Field label={t("meetings.field.endDate")} value={endDateOnly} />
        <Field label={t("meetings.field.allDay")} value={detail.all_day ? "✓" : "—"} />
        <Field label={t("meetings.field.recurrence")} value={t(`meetings.recurrence.${detail.recurrence}`)} />
        <Field label={t("meetings.field.timezone")} value={`時區：${detail.timezone}`} />
      </div>
      <div className="mt-4">
        <Field label={t("meetings.field.location")} value={detail.location ?? "—"} fullWidth />
      </div>
      <div className="mt-4">
        <Field
          label={t("meetings.field.attendees")}
          value={detail.attendees.map((a) => a.display_name || a.email).join("、")}
          fullWidth
        />
      </div>
      {detail.notification_note && (
        <div className="mt-4">
          <Field
            label={t("meetings.field.notificationNote")}
            value={detail.notification_note}
            fullWidth
          />
        </div>
      )}

      {detail.linked_project && (
        <div className="mt-4 rounded-lg bg-[#EFF6FF] px-3 py-2 text-[13px] text-[#0050A0]">
          {t("meetings.linkedProject")} <strong>{detail.linked_project.name}</strong>
        </div>
      )}
    </div>
  );
}

function Field({
  label,
  value,
  span,
  fullWidth,
}: {
  label: string;
  value: string;
  span?: number;
  fullWidth?: boolean;
}) {
  return (
    <div className={fullWidth ? "col-span-3" : span === 2 ? "col-span-2" : ""}>
      <div className="text-[12px] font-medium text-[#475569]">{label}</div>
      <div className="mt-1 rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-2 text-[14px] text-[#1A1A2E]">
        {value}
      </div>
    </div>
  );
}

function RecordTab({
  detail,
  onChange,
}: {
  detail: MeetingDetail;
  onChange: () => void;
}) {
  return (
    <>
      <section className="flex-1 space-y-5">
        <NotesSummary meetingId={detail.id} notes={detail.latest_notes} onChange={onChange} />
      </section>
      <aside className="flex w-[400px] flex-shrink-0 flex-col gap-5 overflow-y-auto">
        <AttendeeSignoff meetingId={detail.id} attendees={detail.attendees} onChange={onChange} />
        <TaskImpactList
          meetingId={detail.id}
          impacts={detail.task_impacts}
          defaultProjectId={detail.project_id}
          onChange={onChange}
        />
        <NotesHistory meetingId={detail.id} />
      </aside>
    </>
  );
}
