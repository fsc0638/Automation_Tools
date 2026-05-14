"use client";
/* eslint-disable react-hooks/set-state-in-effect */
import { useEffect, useState } from "react";
import {
  meetings as meetingsApi,
  projects as projectsApi,
  userViews,
  type MeetingImpactType,
  type Project,
  type UserTask,
} from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export function AddTaskImpactModal({
  meetingId,
  open,
  defaultProjectId,
  onClose,
  onAdded,
}: {
  meetingId: string;
  open: boolean;
  defaultProjectId: string | null;
  onClose: () => void;
  onAdded: () => void;
}) {
  const t = useT();
  const [impactType, setImpactType] = useState<MeetingImpactType>("new");
  const [description, setDescription] = useState("");
  const [projectId, setProjectId] = useState<string | null>(defaultProjectId);
  const [taskId, setTaskId] = useState<string | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [tasks, setTasks] = useState<UserTask[]>([]);
  const [progressFrom, setProgressFrom] = useState<string>("");
  const [progressTo, setProgressTo] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!open) return;
    (async () => {
      try {
        setProjects(await projectsApi.list());
      } catch {
        setProjects([]);
      }
    })();
  }, [open]);

  useEffect(() => {
    if (!projectId) {
      setTasks([]);
      return;
    }
    (async () => {
      try {
        const ts = await userViews.tasks({ projectId });
        setTasks(ts);
      } catch {
        setTasks([]);
      }
    })();
  }, [projectId]);

  if (!open) return null;

  async function submit() {
    setError("");
    if (!description.trim()) {
      setError("請填寫變更描述");
      return;
    }
    if (impactType === "progress" && (!progressFrom || !progressTo)) {
      setError("進度類型需填寫變更前後 %");
      return;
    }
    setBusy(true);
    try {
      await meetingsApi.addTaskImpact(meetingId, {
        project_id: projectId,
        task_id: taskId,
        impact_type: impactType,
        description: description.trim(),
        progress_from: progressFrom ? Number(progressFrom) : undefined,
        progress_to: progressTo ? Number(progressTo) : undefined,
      });
      onAdded();
      onClose();
      // Reset for next open
      setDescription("");
      setProgressFrom("");
      setProgressTo("");
      setTaskId(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : "新增失敗");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="w-full max-w-[480px] rounded-2xl bg-white p-5 shadow-xl">
        <div className="mb-4 text-[18px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
          新增任務影響
        </div>

        <div className="space-y-3">
          <div>
            <label className="block text-[12px] font-medium text-[#475569]">類型</label>
            <div className="mt-1 grid grid-cols-3 gap-2">
              {(["new", "update", "progress"] as const).map((k) => (
                <button
                  key={k}
                  type="button"
                  onClick={() => setImpactType(k)}
                  className={cn(
                    "rounded-xl border px-3 py-2 text-[13px] transition",
                    impactType === k
                      ? "border-[#1A1A2E] bg-[#1A1A2E] text-white"
                      : "border-[#E2E8F0] bg-white text-[#475569] hover:bg-[#F8FAFC]"
                  )}
                >
                  {t(`meetings.impacts.type.${k}`)}
                </button>
              ))}
            </div>
          </div>

          <div>
            <label className="block text-[12px] font-medium text-[#475569]">專案</label>
            <select
              value={projectId ?? ""}
              onChange={(e) => {
                setProjectId(e.target.value || null);
                setTaskId(null);
              }}
              className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[13px] focus:border-[#0050A0] focus:outline-none"
            >
              <option value="">（不指定專案）</option>
              {projects.map((p) => (
                <option key={p.id} value={p.id}>{p.name}</option>
              ))}
            </select>
          </div>

          {projectId && tasks.length > 0 && (
            <div>
              <label className="block text-[12px] font-medium text-[#475569]">任務（選填）</label>
              <select
                value={taskId ?? ""}
                onChange={(e) => setTaskId(e.target.value || null)}
                className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[13px] focus:border-[#0050A0] focus:outline-none"
              >
                <option value="">（不指定任務，僅描述）</option>
                {tasks.map((t) => (
                  <option key={t.id} value={t.id}>{t.title}</option>
                ))}
              </select>
            </div>
          )}

          <div>
            <label className="block text-[12px] font-medium text-[#475569]">變更描述</label>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={3}
              placeholder="例：建立 Notifications / email fallback 說明"
              className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
            />
          </div>

          {impactType === "progress" && (
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="block text-[12px] font-medium text-[#475569]">變更前 %</label>
                <input
                  type="number"
                  min={0}
                  max={100}
                  value={progressFrom}
                  onChange={(e) => setProgressFrom(e.target.value)}
                  className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                />
              </div>
              <div>
                <label className="block text-[12px] font-medium text-[#475569]">變更後 %</label>
                <input
                  type="number"
                  min={0}
                  max={100}
                  value={progressTo}
                  onChange={(e) => setProgressTo(e.target.value)}
                  className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                />
              </div>
            </div>
          )}
        </div>

        {error && (
          <div className="mt-3 rounded-md bg-[#FEE2E2] px-2 py-1.5 text-[12px] text-[#991B1B]">{error}</div>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-xl border border-[#E2E8F0] bg-white px-3.5 py-2 text-[13px] font-medium text-[#475569] hover:bg-[#F8FAFC]"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void submit()}
            disabled={busy}
            className="rounded-xl bg-[#1A1A2E] px-3.5 py-2 text-[13px] font-medium text-white hover:bg-[#243149] disabled:opacity-60"
          >
            {busy ? "建立中…" : "新增"}
          </button>
        </div>
      </div>
    </div>
  );
}
