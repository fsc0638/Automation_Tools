"use client";
/**
 * Edit-existing-meeting page. Loads the meeting, lets the creator
 * update the editable subset of fields, PATCHes back via /meetings/:id.
 *
 * The "Create" page (../new/page.tsx) is the heavyweight form; this one
 * is a tighter subset because some fields shouldn't change after a
 * meeting was scheduled (e.g. portal-side reservation key is tied to
 * room+time, so changing them safely needs a re-book + cancel — deferred).
 *
 * Auth: backend already 403s non-creators on PATCH; we additionally
 * bounce them back at mount time to avoid wasting a request.
 */
import { use, useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { ArrowLeft } from "lucide-react";
import { MeetingSidebar } from "@/components/meetings/MeetingSidebar";
import {
  meetings as meetingsApi,
  projects as projectsApi,
  type MeetingDetail,
  type MeetingImportance,
} from "@/lib/api";
import { useAuthStore } from "@/lib/store";

function toLocalDateInput(iso: string): string {
  const d = new Date(iso);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}
function toLocalTimeInput(iso: string): string {
  const d = new Date(iso);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}
function fromLocal(dateStr: string, timeStr: string): Date | null {
  const m = dateStr.match(/^(\d{4})-(\d{1,2})-(\d{1,2})$/);
  const t = timeStr.match(/^(\d{1,2}):(\d{2})$/);
  if (!m || !t) return null;
  return new Date(
    parseInt(m[1]),
    parseInt(m[2]) - 1,
    parseInt(m[3]),
    parseInt(t[1]),
    parseInt(t[2])
  );
}

export default function EditMeetingPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const router = useRouter();
  const { id } = use(params);
  const currentUser = useAuthStore((s) => s.user);

  const [detail, setDetail] = useState<MeetingDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  // Editable state — initialised from detail on first load.
  const [title, setTitle] = useState("");
  const [importance, setImportance] = useState<MeetingImportance>("normal");
  const [startDate, setStartDate] = useState("");
  const [startTime, setStartTime] = useState("");
  const [endDate, setEndDate] = useState("");
  const [endTime, setEndTime] = useState("");
  const [location, setLocation] = useState("");
  const [description, setDescription] = useState("");
  const [joinUrl, setJoinUrl] = useState("");
  const [notificationNote, setNotificationNote] = useState("");
  const [projectId, setProjectId] = useState<string | null>(null);
  const [allProjects, setAllProjects] = useState<Array<{ id: string; name: string }>>([]);

  useEffect(() => {
    (async () => {
      try {
        const d = await meetingsApi.get(id);
        setDetail(d);
        setTitle(d.title);
        setImportance(d.importance);
        setStartDate(toLocalDateInput(d.start_at));
        setStartTime(toLocalTimeInput(d.start_at));
        setEndDate(toLocalDateInput(d.end_at));
        setEndTime(toLocalTimeInput(d.end_at));
        setLocation(d.location ?? "");
        setDescription(d.description ?? "");
        setJoinUrl(d.join_url ?? "");
        setNotificationNote(d.notification_note ?? "");
        setProjectId(d.project_id);
      } catch (e) {
        setError(e instanceof Error ? e.message : "讀取失敗");
      } finally {
        setLoading(false);
      }
    })();
  }, [id]);

  useEffect(() => {
    (async () => {
      try {
        const list = await projectsApi.list();
        setAllProjects(list.map((p) => ({ id: p.id, name: p.name })));
      } catch {
        /* ignore */
      }
    })();
  }, []);

  // Bounce non-creators back to the read-only detail page. Backend would
  // 403 the PATCH anyway, but front-loading the redirect makes intent
  // clear and matches the brief: "非自己建立的會議...不可以異動".
  useEffect(() => {
    if (!loading && detail && currentUser && detail.creator_id !== currentUser.id) {
      router.replace(`/meetings/${id}`);
    }
  }, [loading, detail, currentUser, router, id]);

  const save = useCallback(async () => {
    setError("");
    const startAt = fromLocal(startDate, startTime);
    const endAt = fromLocal(endDate, endTime);
    if (!title.trim()) {
      setError("會議名稱必填");
      return;
    }
    if (!startAt || !endAt) {
      setError("日期或時間格式不正確");
      return;
    }
    if (endAt <= startAt) {
      setError("結束時間需在開始時間之後");
      return;
    }
    setBusy(true);
    try {
      await meetingsApi.update(id, {
        title: title.trim(),
        importance,
        start_at: startAt.toISOString(),
        end_at: endAt.toISOString(),
        location: location.trim() || null,
        description: description.trim() || null,
        join_url: joinUrl.trim() || null,
        notification_note: notificationNote.trim() || null,
        project_id: projectId,
      });
      router.push(`/meetings/${id}`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : "儲存失敗";
      setError(/forbidden|403/i.test(msg) ? "您沒有編輯此會議的權限" : msg);
      setBusy(false);
    }
  }, [id, title, importance, startDate, startTime, endDate, endTime,
      location, description, joinUrl, notificationNote, projectId, router]);

  if (loading) {
    return (
      <div className="flex h-screen">
        <MeetingSidebar activeMeetingId={id} />
        <div className="flex flex-1 items-center justify-center text-[#94A3B8]">Loading…</div>
      </div>
    );
  }
  if (error && !detail) {
    return (
      <div className="flex h-screen">
        <MeetingSidebar activeMeetingId={id} />
        <div className="flex flex-1 items-center justify-center text-[#C8102E]">{error}</div>
      </div>
    );
  }

  return (
    <div className="flex h-screen overflow-hidden">
      <MeetingSidebar activeMeetingId={id} />
      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <header className="flex items-start justify-between border-b border-[#E2E8F0] bg-white px-6 py-4">
          <div className="flex items-start gap-3">
            <button
              type="button"
              onClick={() => router.push(`/meetings/${id}`)}
              aria-label="回會議詳情"
              className="mt-0.5 inline-flex h-8 w-8 items-center justify-center rounded-lg border border-[#E2E8F0] bg-white text-[#475569] hover:bg-[#F8FAFC]"
            >
              <ArrowLeft size={16} />
            </button>
            <div>
              <h1 className="text-[20px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
                編輯會議
              </h1>
              <p className="mt-1 text-[12px] text-[#94A3B8]">
                修改基本資料；改完按右上「儲存」生效。地點 / 時間異動目前不會自動重新預約凱衛 — 需重訂請刪除後重建。
              </p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => router.push(`/meetings/${id}`)}
              className="rounded-xl border border-[#E2E8F0] bg-white px-3.5 py-2 text-[13px] font-medium text-[#1A1A2E] hover:bg-[#F8FAFC]"
            >
              取消
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void save()}
              className="rounded-xl bg-[#1A1A2E] px-3.5 py-2 text-[13px] font-medium text-white hover:bg-[#243149] disabled:opacity-60"
            >
              {busy ? "儲存中…" : "儲存"}
            </button>
          </div>
        </header>

        {error && (
          <div className="border-b border-[#FCA5A5] bg-[#FEE2E2] px-6 py-2 text-[13px] text-[#991B1B]">
            {error}
          </div>
        )}

        <div className="flex-1 overflow-auto p-6">
          <div className="mx-auto max-w-3xl space-y-5">
            <div className="rounded-2xl border border-[#E2E8F0] bg-white p-3">
              <label className="block text-[12px] font-medium text-[#475569]">
                連結專案
              </label>
              <select
                value={projectId ?? ""}
                onChange={(e) => setProjectId(e.target.value || null)}
                className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
              >
                <option value="">— 不連結專案 —</option>
                {allProjects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </div>

            <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
              <div className="mb-4 text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
                會議基本資料
              </div>

              <div className="grid grid-cols-3 gap-4">
                <div className="col-span-2">
                  <label className="block text-[12px] font-medium text-[#475569]">會議名稱</label>
                  <input
                    type="text"
                    value={title}
                    onChange={(e) => setTitle(e.target.value)}
                    className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                  />
                </div>
                <div>
                  <label className="block text-[12px] font-medium text-[#475569]">重要程度</label>
                  <select
                    value={importance}
                    onChange={(e) => setImportance(e.target.value as MeetingImportance)}
                    className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                  >
                    <option value="normal">一般</option>
                    <option value="important">重要</option>
                  </select>
                </div>
              </div>

              <div className="mt-4 grid grid-cols-4 gap-4">
                <div>
                  <label className="block text-[12px] font-medium text-[#475569]">開始日期</label>
                  <input
                    type="date"
                    value={startDate}
                    onChange={(e) => setStartDate(e.target.value)}
                    className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                  />
                </div>
                <div>
                  <label className="block text-[12px] font-medium text-[#475569]">開始時間</label>
                  <input
                    type="time"
                    step={300}
                    value={startTime}
                    onChange={(e) => setStartTime(e.target.value)}
                    className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                  />
                </div>
                <div>
                  <label className="block text-[12px] font-medium text-[#475569]">結束時間</label>
                  <input
                    type="time"
                    step={300}
                    value={endTime}
                    onChange={(e) => setEndTime(e.target.value)}
                    className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                  />
                </div>
                <div>
                  <label className="block text-[12px] font-medium text-[#475569]">結束日期</label>
                  <input
                    type="date"
                    value={endDate}
                    onChange={(e) => setEndDate(e.target.value)}
                    className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                  />
                </div>
              </div>

              <div className="mt-4">
                <label className="block text-[12px] font-medium text-[#475569]">地點</label>
                <input
                  type="text"
                  value={location}
                  onChange={(e) => setLocation(e.target.value)}
                  className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                />
              </div>

              <div className="mt-4">
                <label className="block text-[12px] font-medium text-[#475569]">線上會議連結</label>
                <input
                  type="url"
                  value={joinUrl}
                  onChange={(e) => setJoinUrl(e.target.value)}
                  placeholder="https://..."
                  className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                />
              </div>

              <div className="mt-4">
                <label className="block text-[12px] font-medium text-[#475569]">會議介紹</label>
                <textarea
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  rows={2}
                  placeholder="會議內容、議題或會議目標…"
                  className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                />
              </div>

              <div className="mt-4">
                <label className="block text-[12px] font-medium text-[#475569]">通知說明</label>
                <textarea
                  value={notificationNote}
                  onChange={(e) => setNotificationNote(e.target.value)}
                  rows={2}
                  className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
