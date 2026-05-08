"use client";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ExternalLink, Plus, Trash2, X } from "lucide-react";
import { tasks as tasksApi, type ProjectTask, type TaskPriority, type TaskStatus, type UpdateTaskInput } from "@/lib/api";
import { useT } from "@/lib/i18n";

const PRIORITY_BADGE: Record<TaskPriority, string> = {
  critical: "bg-red-100 text-red-700",
  high: "bg-orange-100 text-orange-700",
  medium: "bg-yellow-100 text-yellow-700",
  low: "bg-slate-100 text-slate-600",
};

const STATUS_NEXT: Record<TaskStatus, TaskStatus | null> = {
  todo: "in-progress",
  "in-progress": "done",
  done: null,
  cancelled: null,
};

export interface RoadmapTabProps {
  projectId: string;
  /** When set, clicking the source-message link on a task opens that
   *  conversation in the workspace tab and scrolls to the message. */
  onOpenSource?: (conversationId: string, messageId: string) => void;
}

export function RoadmapTab({ projectId, onOpenSource }: RoadmapTabProps) {
  const [items, setItems] = useState<ProjectTask[]>([]);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState("");
  const [showNew, setShowNew] = useState(false);
  const [draft, setDraft] = useState({
    title: "",
    why: "",
    priority: "medium" as TaskPriority,
    affected_files: "",
    acceptance_criteria: "",
    estimated_effort: "",
  });
  const [hoverCol, setHoverCol] = useState<TaskStatus | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const t = useT();
  const STATUS_COLUMNS: Array<{ key: TaskStatus; label: string }> = [
    { key: "todo", label: t("roadmap.colTodo") },
    { key: "in-progress", label: t("roadmap.colInProgress") },
    { key: "done", label: t("roadmap.colDone") },
    { key: "cancelled", label: t("roadmap.colCancelled") },
  ];

  const refresh = useCallback(async () => {
    setLoading(true);
    setErr("");
    try {
      setItems(await tasksApi.list(projectId));
    } catch (e) {
      setErr(e instanceof Error ? e.message : "Failed to load tasks");
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  useEffect(() => {
    const timer = window.setTimeout(() => { void refresh(); }, 0);
    return () => window.clearTimeout(timer);
  }, [refresh]);

  async function createTask(e: React.FormEvent) {
    e.preventDefault();
    if (!draft.title.trim()) return;
    const filesArr = draft.affected_files
      .split(/[\s,]+/)
      .map((f) => f.trim())
      .filter(Boolean);
    const created = await tasksApi.create(projectId, {
      title: draft.title.trim(),
      why: draft.why.trim() || undefined,
      priority: draft.priority,
      affected_files: filesArr.length > 0 ? filesArr : undefined,
      acceptance_criteria: draft.acceptance_criteria.trim() || undefined,
      estimated_effort: draft.estimated_effort.trim() || undefined,
    });
    setItems((prev) => [created, ...prev]);
    setDraft({ title: "", why: "", priority: "medium", affected_files: "", acceptance_criteria: "", estimated_effort: "" });
    setShowNew(false);
  }

  const updateTask = useCallback(async (taskId: string, patch: UpdateTaskInput) => {
    const previous = items.find((t) => t.id === taskId);
    setItems((prev) => prev.map((t) => (t.id === taskId ? { ...t, ...patch } as ProjectTask : t)));
    try {
      const updated = await tasksApi.update(projectId, taskId, patch);
      setItems((prev) => prev.map((t) => (t.id === taskId ? updated : t)));
      return updated;
    } catch (e) {
      // revert
      if (previous) setItems((prev) => prev.map((t) => (t.id === taskId ? previous : t)));
      throw e;
    }
  }, [items, projectId]);

  async function moveTask(task: ProjectTask, status: TaskStatus) {
    if (task.status === status) return;
    try {
      await updateTask(task.id, { status });
    } catch (e) {
      alert(e instanceof Error ? e.message : "Failed to move task");
    }
  }

  function onCardDragStart(e: React.DragEvent<HTMLDivElement>, task: ProjectTask) {
    e.dataTransfer.setData("text/task-id", task.id);
    e.dataTransfer.effectAllowed = "move";
  }
  function onColumnDragOver(e: React.DragEvent<HTMLDivElement>, col: TaskStatus) {
    if (e.dataTransfer.types.includes("text/task-id")) {
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      if (hoverCol !== col) setHoverCol(col);
    }
  }
  function onColumnDragLeave(col: TaskStatus) {
    if (hoverCol === col) setHoverCol(null);
  }
  function onColumnDrop(e: React.DragEvent<HTMLDivElement>, col: TaskStatus) {
    e.preventDefault();
    setHoverCol(null);
    const id = e.dataTransfer.getData("text/task-id");
    const task = items.find((t) => t.id === id);
    if (task) void moveTask(task, col);
  }

  async function deleteTask(task: ProjectTask) {
    if (!confirm(`Delete "${task.title}"?`)) return;
    await tasksApi.delete(projectId, task.id);
    setItems((prev) => prev.filter((t) => t.id !== task.id));
    if (activeId === task.id) setActiveId(null);
  }

  const activeTask = useMemo(() => items.find((t) => t.id === activeId) ?? null, [items, activeId]);

  if (loading && items.length === 0) {
    return <div className="p-8 text-center text-[#94A3B8]">{t("common.loading")}</div>;
  }
  if (err) return <div className="p-8 text-center text-[#C8102E]">{err}</div>;

  const grouped: Record<TaskStatus, ProjectTask[]> = {
    todo: [], "in-progress": [], done: [], cancelled: [],
  };
  for (const tk of items) grouped[tk.status]?.push(tk);

  return (
    <div className="p-6 space-y-4 overflow-auto">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-[#1A1A2E]">{t("roadmap.title")}</h2>
          <p className="text-xs text-[#94A3B8]">{t("roadmap.subtitle")}</p>
        </div>
        <div className="flex gap-2">
          <button onClick={() => void refresh()} className="text-xs text-[#0050A0] hover:underline">{t("common.refresh")}</button>
          <button
            onClick={() => setShowNew((v) => !v)}
            className="flex items-center gap-1 px-3 py-1.5 rounded-md bg-[#0050A0] text-white text-xs font-medium hover:bg-[#003B7A]"
          >
            <Plus size={12} /> {t("roadmap.newTask")}
          </button>
        </div>
      </div>

      {showNew && (
        <form onSubmit={createTask} className="rounded-lg border border-[#E2E8F0] bg-white p-4 space-y-3">
          <input
            value={draft.title}
            onChange={(e) => setDraft({ ...draft, title: e.target.value })}
            placeholder={t("roadmap.taskTitle")}
            className="w-full h-10 px-3 rounded-md border border-[#E2E8F0] text-sm"
            required
          />
          <textarea
            value={draft.why}
            onChange={(e) => setDraft({ ...draft, why: e.target.value })}
            placeholder={t("roadmap.whyHint")}
            rows={2}
            className="w-full px-3 py-2 rounded-md border border-[#E2E8F0] text-sm"
          />
          <textarea
            value={draft.acceptance_criteria}
            onChange={(e) => setDraft({ ...draft, acceptance_criteria: e.target.value })}
            placeholder={t("roadmap.acHint")}
            rows={2}
            className="w-full px-3 py-2 rounded-md border border-[#E2E8F0] text-sm"
          />
          <input
            value={draft.affected_files}
            onChange={(e) => setDraft({ ...draft, affected_files: e.target.value })}
            placeholder={t("roadmap.filesHint")}
            className="w-full h-10 px-3 rounded-md border border-[#E2E8F0] text-sm font-mono"
          />
          <div className="flex items-center gap-3">
            <label className="text-xs text-[#64748B]">{t("roadmap.priority")}:</label>
            <select
              value={draft.priority}
              onChange={(e) => setDraft({ ...draft, priority: e.target.value as TaskPriority })}
              className="h-8 px-2 text-xs rounded-md border border-[#E2E8F0]"
            >
              <option value="low">{t("roadmap.priorityLow")}</option>
              <option value="medium">{t("roadmap.priorityMedium")}</option>
              <option value="high">{t("roadmap.priorityHigh")}</option>
              <option value="critical">{t("roadmap.priorityCritical")}</option>
            </select>
            <label className="text-xs text-[#64748B]">{t("roadmap.effort")}:</label>
            <input
              value={draft.estimated_effort}
              onChange={(e) => setDraft({ ...draft, estimated_effort: e.target.value })}
              placeholder="S / M / L / 2d"
              className="h-8 w-24 px-2 text-xs rounded-md border border-[#E2E8F0]"
            />
            <div className="flex-1" />
            <button type="button" onClick={() => setShowNew(false)} className="text-xs text-[#64748B] hover:text-[#1A1A2E]">{t("common.cancel")}</button>
            <button type="submit" className="px-3 py-1.5 rounded-md bg-[#0050A0] text-white text-xs font-medium">{t("common.create")}</button>
          </div>
        </form>
      )}

      {items.length === 0 ? (
        <div className="text-sm text-[#94A3B8] text-center py-12 border border-dashed border-[#E2E8F0] rounded-lg">
          {t("roadmap.empty")}
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-4 gap-3">
          {STATUS_COLUMNS.map((col) => (
            <div
              key={col.key}
              onDragOver={(e) => onColumnDragOver(e, col.key)}
              onDragLeave={() => onColumnDragLeave(col.key)}
              onDrop={(e) => onColumnDrop(e, col.key)}
              className={`bg-[#F8FAFC] rounded-lg p-3 min-h-[200px] border-2 transition-colors ${
                hoverCol === col.key ? "border-[#0050A0] bg-blue-50" : "border-transparent"
              }`}
            >
              <div className="flex items-center justify-between mb-2">
                <span className="text-xs font-semibold text-[#64748B] uppercase tracking-wider">{col.label}</span>
                <span className="text-xs text-[#94A3B8]">{grouped[col.key].length}</span>
              </div>
              <div className="space-y-2">
                {grouped[col.key].map((tk) => (
                  <TaskCard
                    key={tk.id}
                    task={tk}
                    onMove={moveTask}
                    onDelete={deleteTask}
                    onDragStart={onCardDragStart}
                    onOpen={() => setActiveId(tk.id)}
                  />
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {activeTask && (
        <TaskDetailDrawer
          key={activeTask.id}
          task={activeTask}
          onClose={() => setActiveId(null)}
          onUpdate={updateTask}
          onDelete={deleteTask}
          onOpenSource={onOpenSource}
        />
      )}
    </div>
  );
}

function TaskCard({
  task,
  onMove,
  onDelete,
  onDragStart,
  onOpen,
}: {
  task: ProjectTask;
  onMove: (t: ProjectTask, s: TaskStatus) => void | Promise<void>;
  onDelete: (t: ProjectTask) => void | Promise<void>;
  onDragStart: (e: React.DragEvent<HTMLDivElement>, t: ProjectTask) => void;
  onOpen: () => void;
}) {
  const next = STATUS_NEXT[task.status];
  return (
    <div
      draggable
      onDragStart={(e) => onDragStart(e, task)}
      onClick={onOpen}
      className="bg-white border border-[#E2E8F0] rounded-md p-3 group hover:border-[#0050A0] cursor-pointer"
    >
      <div className="flex items-start justify-between gap-2">
        <div className="text-sm font-medium text-[#1A1A2E] flex-1 leading-snug">{task.title}</div>
        <button
          onClick={(e) => { e.stopPropagation(); void onDelete(task); }}
          className="opacity-0 group-hover:opacity-100 text-[#94A3B8] hover:text-[#C8102E]"
          title="Delete"
        >
          <Trash2 size={12} />
        </button>
      </div>
      {task.why && <div className="text-xs text-[#64748B] mt-1 line-clamp-3">{task.why}</div>}
      {task.acceptance_criteria && (
        <div className="mt-2 rounded-sm border-l-2 border-emerald-300 bg-emerald-50/60 px-2 py-1 text-[11px] text-emerald-900 line-clamp-2">
          AC: {task.acceptance_criteria}
        </div>
      )}
      {task.affected_files && task.affected_files.length > 0 && (
        <div className="mt-2 text-[10px] text-[#0050A0] font-mono">
          {task.affected_files.slice(0, 3).join(", ")}
          {task.affected_files.length > 3 && ` +${task.affected_files.length - 3}`}
        </div>
      )}
      <div className="flex items-center gap-2 mt-2">
        <span className={`text-[10px] px-1.5 py-0.5 rounded-full ${PRIORITY_BADGE[task.priority]}`}>
          {task.priority}
        </span>
        {task.estimated_effort && (
          <span className="text-[10px] text-[#94A3B8]">{task.estimated_effort}</span>
        )}
        {task.source_message_id && (
          <span className="text-[10px] text-[#0EA5E9]" title="Has source message">↩</span>
        )}
        <div className="flex-1" />
        {next && (
          <button
            onClick={(e) => { e.stopPropagation(); void onMove(task, next); }}
            className="text-[11px] text-[#0050A0] hover:underline"
          >
            → {next}
          </button>
        )}
        {task.status !== "cancelled" && task.status !== "done" && (
          <button
            onClick={(e) => { e.stopPropagation(); void onMove(task, "cancelled"); }}
            className="text-[11px] text-[#94A3B8] hover:text-[#C8102E]"
            title="Cancel"
          >
            ✕
          </button>
        )}
      </div>
    </div>
  );
}

function TaskDetailDrawer({
  task,
  onClose,
  onUpdate,
  onDelete,
  onOpenSource,
}: {
  task: ProjectTask;
  onClose: () => void;
  onUpdate: (taskId: string, patch: UpdateTaskInput) => Promise<ProjectTask>;
  onDelete: (t: ProjectTask) => void | Promise<void>;
  onOpenSource?: (conversationId: string, messageId: string) => void;
}) {
  const t = useT();
  const [form, setForm] = useState({
    title: task.title,
    why: task.why ?? "",
    acceptance_criteria: task.acceptance_criteria ?? "",
    estimated_effort: task.estimated_effort ?? "",
    affected_files: (task.affected_files ?? []).join(", "),
    priority: task.priority,
    status: task.status,
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const dirty = useMemo(() => {
    const filesArr = form.affected_files.split(/[\s,]+/).map((f) => f.trim()).filter(Boolean);
    const currentFiles = task.affected_files ?? [];
    const filesChanged = filesArr.length !== currentFiles.length || filesArr.some((f, i) => f !== currentFiles[i]);
    return (
      form.title !== task.title ||
      form.why !== (task.why ?? "") ||
      form.acceptance_criteria !== (task.acceptance_criteria ?? "") ||
      form.estimated_effort !== (task.estimated_effort ?? "") ||
      form.priority !== task.priority ||
      form.status !== task.status ||
      filesChanged
    );
  }, [form, task]);

  async function save() {
    if (!form.title.trim()) {
      setError("Title required");
      return;
    }
    setSaving(true);
    setError("");
    try {
      const filesArr = form.affected_files.split(/[\s,]+/).map((f) => f.trim()).filter(Boolean);
      await onUpdate(task.id, {
        title: form.title.trim(),
        why: form.why.trim(),
        acceptance_criteria: form.acceptance_criteria.trim(),
        estimated_effort: form.estimated_effort.trim(),
        affected_files: filesArr,
        priority: form.priority,
        status: form.status,
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : "Save failed");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 z-40 flex justify-end bg-black/30" onClick={onClose}>
      <div
        className="h-full w-full max-w-[560px] overflow-y-auto bg-white shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="sticky top-0 z-10 flex items-center justify-between border-b border-[#E2E8F0] bg-white/95 px-5 py-3 backdrop-blur">
          <div>
            <div className="text-[11px] font-semibold uppercase tracking-[0.14em] text-[#94A3B8]">{t("roadmap.detailTitle")}</div>
            <div className="mt-0.5 text-xs text-[#64748B]">
              {t("roadmap.created")}: {new Date(task.created_at).toLocaleString()}
              {" · "}
              {t("roadmap.updated")}: {new Date(task.updated_at).toLocaleString()}
            </div>
          </div>
          <button onClick={onClose} className="rounded-md p-1.5 text-[#64748B] hover:bg-[#F1F5F9] hover:text-[#1A1A2E]" aria-label="Close">
            <X size={16} />
          </button>
        </div>

        <div className="space-y-4 px-5 py-4">
          <div>
            <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">
              {t("roadmap.taskTitle")}
            </label>
            <input
              value={form.title}
              onChange={(e) => setForm({ ...form, title: e.target.value })}
              className="mt-1 w-full rounded-md border border-[#E2E8F0] px-3 py-2 text-sm font-medium"
            />
          </div>

          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.priority")}</label>
              <select
                value={form.priority}
                onChange={(e) => setForm({ ...form, priority: e.target.value as TaskPriority })}
                className="mt-1 h-9 w-full rounded-md border border-[#E2E8F0] px-2 text-sm"
              >
                <option value="low">{t("roadmap.priorityLow")}</option>
                <option value="medium">{t("roadmap.priorityMedium")}</option>
                <option value="high">{t("roadmap.priorityHigh")}</option>
                <option value="critical">{t("roadmap.priorityCritical")}</option>
              </select>
            </div>
            <div>
              <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.status")}</label>
              <select
                value={form.status}
                onChange={(e) => setForm({ ...form, status: e.target.value as TaskStatus })}
                className="mt-1 h-9 w-full rounded-md border border-[#E2E8F0] px-2 text-sm"
              >
                <option value="todo">{t("roadmap.colTodo")}</option>
                <option value="in-progress">{t("roadmap.colInProgress")}</option>
                <option value="done">{t("roadmap.colDone")}</option>
                <option value="cancelled">{t("roadmap.colCancelled")}</option>
              </select>
            </div>
            <div>
              <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.effort")}</label>
              <input
                value={form.estimated_effort}
                onChange={(e) => setForm({ ...form, estimated_effort: e.target.value })}
                placeholder="S / M / L / 2d"
                className="mt-1 h-9 w-full rounded-md border border-[#E2E8F0] px-2 text-sm"
              />
            </div>
          </div>

          <div>
            <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.why")}</label>
            <textarea
              value={form.why}
              onChange={(e) => setForm({ ...form, why: e.target.value })}
              rows={4}
              className="mt-1 w-full rounded-md border border-[#E2E8F0] px-3 py-2 text-sm leading-6"
              placeholder={t("roadmap.whyHint")}
            />
          </div>

          <div>
            <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.ac")}</label>
            <textarea
              value={form.acceptance_criteria}
              onChange={(e) => setForm({ ...form, acceptance_criteria: e.target.value })}
              rows={4}
              className="mt-1 w-full rounded-md border border-[#E2E8F0] px-3 py-2 text-sm leading-6"
              placeholder={t("roadmap.acHint")}
            />
          </div>

          <div>
            <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.affectedFiles")}</label>
            <textarea
              value={form.affected_files}
              onChange={(e) => setForm({ ...form, affected_files: e.target.value })}
              rows={2}
              className="mt-1 w-full rounded-md border border-[#E2E8F0] px-3 py-2 text-xs font-mono leading-6"
              placeholder={t("roadmap.filesHint")}
            />
            <div className="mt-1 text-[11px] text-[#94A3B8]">{t("roadmap.filesSplitter")}</div>
          </div>

          {task.source_message_id && (
            <div className="rounded-md border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-2 text-xs text-[#475569]">
              <div className="flex items-center justify-between gap-2">
                <span>
                  <span className="font-semibold">{t("roadmap.sourceMsg")}</span>
                  {": "}
                  <span className="font-mono text-[10px] text-[#94A3B8]">{task.source_message_id.slice(0, 8)}…</span>
                </span>
                {task.source_conversation_id && onOpenSource && (
                  <button
                    type="button"
                    onClick={() => onOpenSource(task.source_conversation_id!, task.source_message_id!)}
                    className="inline-flex items-center gap-1 rounded-md border border-[#0050A0] px-2 py-1 text-[11px] text-[#0050A0] hover:bg-[#EFF6FF]"
                  >
                    <ExternalLink size={11} /> {t("roadmap.viewSource")}
                  </button>
                )}
              </div>
            </div>
          )}

          {error && <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">{error}</div>}
        </div>

        <div className="sticky bottom-0 flex items-center justify-between gap-2 border-t border-[#E2E8F0] bg-white/95 px-5 py-3 backdrop-blur">
          <button
            type="button"
            onClick={() => void onDelete(task)}
            className="inline-flex items-center gap-1 rounded-md border border-red-200 px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-50"
          >
            <Trash2 size={12} /> {t("common.delete")}
          </button>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={onClose}
              className="rounded-md px-3 py-1.5 text-xs text-[#64748B] hover:bg-[#F1F5F9] hover:text-[#1A1A2E]"
            >
              {t("common.close")}
            </button>
            <button
              type="button"
              onClick={() => void save()}
              disabled={!dirty || saving}
              className="rounded-md bg-[#0050A0] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#003B7A] disabled:bg-[#94A3B8]"
            >
              {saving ? t("common.loading") : t("common.save")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
