"use client";
import { useCallback, useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { tasks as tasksApi, type ProjectTask, type TaskPriority, type TaskStatus } from "@/lib/api";

const STATUS_COLUMNS: Array<{ key: TaskStatus; label: string }> = [
  { key: "todo", label: "Todo" },
  { key: "in-progress", label: "In progress" },
  { key: "done", label: "Done" },
  { key: "cancelled", label: "Cancelled" },
];

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

export function RoadmapTab({ projectId }: { projectId: string }) {
  const [items, setItems] = useState<ProjectTask[]>([]);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState("");
  const [showNew, setShowNew] = useState(false);
  const [draft, setDraft] = useState({ title: "", why: "", priority: "medium" as TaskPriority });
  const [hoverCol, setHoverCol] = useState<TaskStatus | null>(null);

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
    const created = await tasksApi.create(projectId, {
      title: draft.title.trim(),
      why: draft.why.trim() || undefined,
      priority: draft.priority,
    });
    setItems((prev) => [created, ...prev]);
    setDraft({ title: "", why: "", priority: "medium" });
    setShowNew(false);
  }

  async function moveTask(task: ProjectTask, status: TaskStatus) {
    if (task.status === status) return;
    // Optimistic update so the card snaps to the new column immediately;
    // revert if the server rejects the change.
    const previous = task.status;
    setItems((prev) => prev.map((t) => (t.id === task.id ? { ...t, status } : t)));
    try {
      const updated = await tasksApi.update(projectId, task.id, { status });
      setItems((prev) => prev.map((t) => (t.id === task.id ? updated : t)));
    } catch (e) {
      setItems((prev) => prev.map((t) => (t.id === task.id ? { ...t, status: previous } : t)));
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
  }

  if (loading && items.length === 0) {
    return <div className="p-8 text-center text-[#94A3B8]">Loading…</div>;
  }
  if (err) return <div className="p-8 text-center text-[#C8102E]">{err}</div>;

  const grouped: Record<TaskStatus, ProjectTask[]> = {
    todo: [], "in-progress": [], done: [], cancelled: [],
  };
  for (const t of items) grouped[t.status]?.push(t);

  return (
    <div className="p-6 space-y-4 overflow-auto">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-[#1A1A2E]">Roadmap</h2>
          <p className="text-xs text-[#94A3B8]">Persistent tasks driven from Agent suggestions or added manually.</p>
        </div>
        <div className="flex gap-2">
          <button onClick={() => void refresh()} className="text-xs text-[#0050A0] hover:underline">Refresh</button>
          <button
            onClick={() => setShowNew((v) => !v)}
            className="flex items-center gap-1 px-3 py-1.5 rounded-md bg-[#0050A0] text-white text-xs font-medium hover:bg-[#003B7A]"
          >
            <Plus size={12} /> New task
          </button>
        </div>
      </div>

      {showNew && (
        <form onSubmit={createTask} className="rounded-lg border border-[#E2E8F0] bg-white p-4 space-y-3">
          <input
            value={draft.title}
            onChange={(e) => setDraft({ ...draft, title: e.target.value })}
            placeholder="Task title…"
            className="w-full h-10 px-3 rounded-md border border-[#E2E8F0] text-sm"
            required
          />
          <textarea
            value={draft.why}
            onChange={(e) => setDraft({ ...draft, why: e.target.value })}
            placeholder="Why does this matter? (optional)"
            rows={2}
            className="w-full px-3 py-2 rounded-md border border-[#E2E8F0] text-sm"
          />
          <div className="flex items-center gap-3">
            <label className="text-xs text-[#64748B]">Priority:</label>
            <select
              value={draft.priority}
              onChange={(e) => setDraft({ ...draft, priority: e.target.value as TaskPriority })}
              className="h-8 px-2 text-xs rounded-md border border-[#E2E8F0]"
            >
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
              <option value="critical">Critical</option>
            </select>
            <div className="flex-1" />
            <button type="button" onClick={() => setShowNew(false)} className="text-xs text-[#64748B] hover:text-[#1A1A2E]">Cancel</button>
            <button type="submit" className="px-3 py-1.5 rounded-md bg-[#0050A0] text-white text-xs font-medium">Create</button>
          </div>
        </form>
      )}

      {items.length === 0 ? (
        <div className="text-sm text-[#94A3B8] text-center py-12 border border-dashed border-[#E2E8F0] rounded-lg">
          No tasks yet. Click <span className="text-[#0050A0]">New task</span> or run a Patch Plan / Roadmap quick action.
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
                {grouped[col.key].map((t) => (
                  <TaskCard
                    key={t.id}
                    task={t}
                    onMove={moveTask}
                    onDelete={deleteTask}
                    onDragStart={onCardDragStart}
                  />
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function TaskCard({
  task,
  onMove,
  onDelete,
  onDragStart,
}: {
  task: ProjectTask;
  onMove: (t: ProjectTask, s: TaskStatus) => void | Promise<void>;
  onDelete: (t: ProjectTask) => void | Promise<void>;
  onDragStart: (e: React.DragEvent<HTMLDivElement>, t: ProjectTask) => void;
}) {
  const next = STATUS_NEXT[task.status];
  return (
    <div
      draggable
      onDragStart={(e) => onDragStart(e, task)}
      className="bg-white border border-[#E2E8F0] rounded-md p-3 group hover:border-[#94A3B8] cursor-grab active:cursor-grabbing"
    >
      <div className="flex items-start justify-between gap-2">
        <div className="text-sm font-medium text-[#1A1A2E] flex-1 leading-snug">{task.title}</div>
        <button
          onClick={() => void onDelete(task)}
          className="opacity-0 group-hover:opacity-100 text-[#94A3B8] hover:text-[#C8102E]"
          title="Delete"
        >
          <Trash2 size={12} />
        </button>
      </div>
      {task.why && <div className="text-xs text-[#64748B] mt-1 line-clamp-3">{task.why}</div>}
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
        <div className="flex-1" />
        {next && (
          <button
            onClick={() => void onMove(task, next)}
            className="text-[11px] text-[#0050A0] hover:underline"
          >
            → {next}
          </button>
        )}
        {task.status !== "cancelled" && task.status !== "done" && (
          <button
            onClick={() => void onMove(task, "cancelled")}
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
