"use client";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ExternalLink, Filter, GitBranch, GitPullRequest, History, Lock, Plus, Search, Send, Tag, Trash2, X } from "lucide-react";
import {
  tasks as tasksApi,
  type AcceptanceCriteriaV2,
  type ProjectTask,
  type TaskAttempt,
  type TaskPriority,
  type TaskStatus,
  type TaskStatusEvent,
  type UpdateTaskInput,
} from "@/lib/api";
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

const PRIORITY_RANK: Record<TaskPriority, number> = {
  critical: 0, high: 1, medium: 2, low: 3,
};

type SortKey = "priority" | "due" | "newest" | "oldest" | "updated";

export interface RoadmapTabProps {
  projectId: string;
  /** When set, clicking the source-message link on a task opens that
   *  conversation in the workspace tab and scrolls to the message. */
  onOpenSource?: (conversationId: string, messageId: string) => void;
  /** When set, a "Send to Agent" button appears in the drawer. The page
   *  is responsible for switching to the workspace tab, opening the
   *  conversation, and pre-filling the composer with `prompt`. */
  onDispatched?: (conversationId: string, prompt: string) => void;
}

export function RoadmapTab({ projectId, onOpenSource, onDispatched }: RoadmapTabProps) {
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
    assignee: "",
    due_date: "",
    labels: "",
  });
  const [hoverCol, setHoverCol] = useState<TaskStatus | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);

  // Filter / sort state
  const [search, setSearch] = useState("");
  const [filterPriority, setFilterPriority] = useState<TaskPriority | "all">("all");
  const [filterAssignee, setFilterAssignee] = useState<string>("all");
  const [filterLabel, setFilterLabel] = useState<string>("all");
  const [filterOverdue, setFilterOverdue] = useState(false);
  const [sortKey, setSortKey] = useState<SortKey>("priority");

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
    const filesArr = draft.affected_files.split(/[\s,]+/).map((f) => f.trim()).filter(Boolean);
    const labelsArr = draft.labels.split(/[\s,]+/).map((l) => l.trim()).filter(Boolean);
    const created = await tasksApi.create(projectId, {
      title: draft.title.trim(),
      why: draft.why.trim() || undefined,
      priority: draft.priority,
      affected_files: filesArr.length > 0 ? filesArr : undefined,
      acceptance_criteria: draft.acceptance_criteria.trim() || undefined,
      estimated_effort: draft.estimated_effort.trim() || undefined,
      assignee: draft.assignee.trim() || undefined,
      due_date: draft.due_date || undefined,
      labels: labelsArr.length > 0 ? labelsArr : undefined,
    });
    setItems((prev) => [created, ...prev]);
    setDraft({ title: "", why: "", priority: "medium", affected_files: "", acceptance_criteria: "", estimated_effort: "", assignee: "", due_date: "", labels: "" });
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

  const knownAssignees = useMemo(() => {
    const set = new Set<string>();
    for (const t of items) if (t.assignee && t.assignee.trim()) set.add(t.assignee.trim());
    return Array.from(set).sort();
  }, [items]);

  const knownLabels = useMemo(() => {
    const set = new Set<string>();
    for (const t of items) for (const l of t.labels ?? []) if (l) set.add(l);
    return Array.from(set).sort();
  }, [items]);

  const filteredItems = useMemo(() => {
    const today = new Date(); today.setHours(0, 0, 0, 0);
    const q = search.trim().toLowerCase();
    let arr = items.filter((t) => {
      if (filterPriority !== "all" && t.priority !== filterPriority) return false;
      if (filterAssignee !== "all" && (t.assignee ?? "") !== filterAssignee) return false;
      if (filterLabel !== "all" && !(t.labels ?? []).includes(filterLabel)) return false;
      if (filterOverdue) {
        if (!t.due_date) return false;
        if (t.status === "done" || t.status === "cancelled") return false;
        const due = new Date(t.due_date); due.setHours(0, 0, 0, 0);
        if (due >= today) return false;
      }
      if (q) {
        const hay = [t.title, t.why ?? "", t.acceptance_criteria ?? "", (t.affected_files ?? []).join(" "), (t.labels ?? []).join(" "), t.assignee ?? ""].join(" ").toLowerCase();
        if (!hay.includes(q)) return false;
      }
      return true;
    });
    arr = arr.slice().sort((a, b) => {
      switch (sortKey) {
        case "priority":
          return (PRIORITY_RANK[a.priority] - PRIORITY_RANK[b.priority]) ||
            (b.created_at.localeCompare(a.created_at));
        case "due": {
          const ad = a.due_date ? new Date(a.due_date).getTime() : Number.POSITIVE_INFINITY;
          const bd = b.due_date ? new Date(b.due_date).getTime() : Number.POSITIVE_INFINITY;
          return ad - bd;
        }
        case "newest": return b.created_at.localeCompare(a.created_at);
        case "oldest": return a.created_at.localeCompare(b.created_at);
        case "updated": return b.updated_at.localeCompare(a.updated_at);
        default: return 0;
      }
    });
    return arr;
  }, [items, search, filterPriority, filterAssignee, filterLabel, filterOverdue, sortKey]);

  const activeTask = useMemo(() => items.find((t) => t.id === activeId) ?? null, [items, activeId]);

  if (loading && items.length === 0) {
    return <div className="p-8 text-center text-[#94A3B8]">{t("common.loading")}</div>;
  }
  if (err) return <div className="p-8 text-center text-[#C8102E]">{err}</div>;

  const grouped: Record<TaskStatus, ProjectTask[]> = {
    todo: [], "in-progress": [], done: [], cancelled: [],
  };
  for (const tk of filteredItems) grouped[tk.status]?.push(tk);

  const filtersActive = filterPriority !== "all" || filterAssignee !== "all" || filterLabel !== "all" || filterOverdue || search.trim().length > 0;

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

      {/* Toolbar: search + filters + sort */}
      <div className="rounded-lg border border-[#E2E8F0] bg-white p-3 space-y-2">
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex flex-1 min-w-[220px] items-center gap-2 rounded-md border border-[#E2E8F0] px-2 py-1.5">
            <Search size={13} className="text-[#94A3B8]" />
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("roadmap.searchPlaceholder")}
              className="flex-1 bg-transparent text-xs outline-none placeholder:text-[#94A3B8]"
            />
            {search && (
              <button onClick={() => setSearch("")} className="text-[#94A3B8] hover:text-[#1A1A2E]">
                <X size={12} />
              </button>
            )}
          </div>
          <select
            value={sortKey}
            onChange={(e) => setSortKey(e.target.value as SortKey)}
            className="h-8 rounded-md border border-[#E2E8F0] bg-white px-2 text-xs"
            title={t("roadmap.sortBy")}
          >
            <option value="priority">{t("roadmap.sortPriority")}</option>
            <option value="due">{t("roadmap.sortDue")}</option>
            <option value="newest">{t("roadmap.sortNewest")}</option>
            <option value="oldest">{t("roadmap.sortOldest")}</option>
            <option value="updated">{t("roadmap.sortUpdated")}</option>
          </select>
        </div>
        <div className="flex flex-wrap items-center gap-2 text-[11px]">
          <Filter size={11} className="text-[#94A3B8]" />
          <select
            value={filterPriority}
            onChange={(e) => setFilterPriority(e.target.value as TaskPriority | "all")}
            className="h-7 rounded-md border border-[#E2E8F0] bg-white px-2"
          >
            <option value="all">{t("roadmap.priority")}: {t("roadmap.allPriorities")}</option>
            <option value="critical">{t("roadmap.priority")}: {t("roadmap.priorityCritical")}</option>
            <option value="high">{t("roadmap.priority")}: {t("roadmap.priorityHigh")}</option>
            <option value="medium">{t("roadmap.priority")}: {t("roadmap.priorityMedium")}</option>
            <option value="low">{t("roadmap.priority")}: {t("roadmap.priorityLow")}</option>
          </select>
          <select
            value={filterAssignee}
            onChange={(e) => setFilterAssignee(e.target.value)}
            className="h-7 rounded-md border border-[#E2E8F0] bg-white px-2"
          >
            <option value="all">{t("roadmap.assignee")}: {t("roadmap.allAssignees")}</option>
            <option value="">{t("roadmap.unassigned")}</option>
            {knownAssignees.map((a) => <option key={a} value={a}>{a}</option>)}
          </select>
          <select
            value={filterLabel}
            onChange={(e) => setFilterLabel(e.target.value)}
            className="h-7 rounded-md border border-[#E2E8F0] bg-white px-2"
            disabled={knownLabels.length === 0}
          >
            <option value="all">{t("roadmap.labels")}: {t("roadmap.allLabels")}</option>
            {knownLabels.map((l) => <option key={l} value={l}>{l}</option>)}
          </select>
          <label className="flex items-center gap-1 rounded-md border border-[#E2E8F0] bg-white px-2 py-1">
            <input
              type="checkbox"
              checked={filterOverdue}
              onChange={(e) => setFilterOverdue(e.target.checked)}
              className="h-3 w-3"
            />
            {t("roadmap.onlyOverdue")}
          </label>
          {filtersActive && (
            <button
              onClick={() => {
                setSearch(""); setFilterPriority("all"); setFilterAssignee("all"); setFilterLabel("all"); setFilterOverdue(false);
              }}
              className="text-[11px] text-[#0050A0] hover:underline"
            >
              {t("roadmap.clearFilters")}
            </button>
          )}
          <span className="ml-auto text-[#94A3B8]">
            {filteredItems.length} / {items.length}
          </span>
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
          <div className="grid grid-cols-2 md:grid-cols-3 gap-2">
            <input
              value={draft.assignee}
              onChange={(e) => setDraft({ ...draft, assignee: e.target.value })}
              placeholder={t("roadmap.assigneeHint")}
              className="h-9 px-3 rounded-md border border-[#E2E8F0] text-xs"
            />
            <input
              type="date"
              value={draft.due_date}
              onChange={(e) => setDraft({ ...draft, due_date: e.target.value })}
              className="h-9 px-3 rounded-md border border-[#E2E8F0] text-xs"
            />
            <input
              value={draft.labels}
              onChange={(e) => setDraft({ ...draft, labels: e.target.value })}
              placeholder={t("roadmap.labelsHint")}
              className="h-9 px-3 rounded-md border border-[#E2E8F0] text-xs"
            />
          </div>
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

      {filteredItems.length === 0 ? (
        <div className="text-sm text-[#94A3B8] text-center py-12 border border-dashed border-[#E2E8F0] rounded-lg">
          {items.length === 0 ? t("roadmap.empty") : t("roadmap.noMatch")}
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
                    allTasks={items}
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
          allTasks={items}
          projectId={projectId}
          onClose={() => setActiveId(null)}
          onUpdate={updateTask}
          onDelete={deleteTask}
          onOpenSource={onOpenSource}
          onDispatched={onDispatched}
        />
      )}
    </div>
  );
}

/** Open dependencies that are not yet done/cancelled — i.e. real blockers. */
function findBlockers(task: ProjectTask, all: ProjectTask[]): ProjectTask[] {
  if (!task.depends_on || task.depends_on.length === 0) return [];
  const map = new Map(all.map((t) => [t.id, t]));
  return task.depends_on
    .map((id) => map.get(id))
    .filter((t): t is ProjectTask => !!t && t.status !== "done" && t.status !== "cancelled");
}

function isOverdue(task: ProjectTask): boolean {
  if (!task.due_date) return false;
  if (task.status === "done" || task.status === "cancelled") return false;
  const today = new Date(); today.setHours(0, 0, 0, 0);
  const due = new Date(task.due_date); due.setHours(0, 0, 0, 0);
  return due < today;
}

function formatDueRel(due: string): string {
  const today = new Date(); today.setHours(0, 0, 0, 0);
  const d = new Date(due); d.setHours(0, 0, 0, 0);
  const diff = Math.round((d.getTime() - today.getTime()) / 86400000);
  if (diff === 0) return "today";
  if (diff === -1) return "1d overdue";
  if (diff < 0) return `${-diff}d overdue`;
  if (diff === 1) return "tomorrow";
  return `${diff}d`;
}

function TaskCard({
  task,
  allTasks,
  onMove,
  onDelete,
  onDragStart,
  onOpen,
}: {
  task: ProjectTask;
  allTasks: ProjectTask[];
  onMove: (t: ProjectTask, s: TaskStatus) => void | Promise<void>;
  onDelete: (t: ProjectTask) => void | Promise<void>;
  onDragStart: (e: React.DragEvent<HTMLDivElement>, t: ProjectTask) => void;
  onOpen: () => void;
}) {
  const next = STATUS_NEXT[task.status];
  const overdue = isOverdue(task);
  const blockers = findBlockers(task, allTasks);
  const blocked = blockers.length > 0 && task.status !== "done" && task.status !== "cancelled";
  return (
    <div
      draggable
      onDragStart={(e) => onDragStart(e, task)}
      onClick={onOpen}
      className={`bg-white border rounded-md p-3 group hover:border-[#0050A0] cursor-pointer ${overdue ? "border-red-300" : "border-[#E2E8F0]"}`}
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
      {task.labels && task.labels.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1">
          {task.labels.slice(0, 4).map((l) => (
            <span key={l} className="inline-flex items-center gap-0.5 rounded-full bg-[#EEF2FF] px-1.5 py-0.5 text-[10px] text-[#3730A3]">
              <Tag size={9} /> {l}
            </span>
          ))}
          {task.labels.length > 4 && (
            <span className="text-[10px] text-[#94A3B8]">+{task.labels.length - 4}</span>
          )}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-1.5 mt-2">
        <span className={`text-[10px] px-1.5 py-0.5 rounded-full ${PRIORITY_BADGE[task.priority]}`}>
          {task.priority}
        </span>
        {task.estimated_effort && (
          <span className="text-[10px] text-[#94A3B8]">{task.estimated_effort}</span>
        )}
        {task.assignee && (
          <span className="text-[10px] rounded-full bg-slate-100 px-1.5 py-0.5 text-slate-700">
            @{task.assignee}
          </span>
        )}
        {task.due_date && (
          <span className={`text-[10px] rounded-full px-1.5 py-0.5 ${overdue ? "bg-red-100 text-red-700" : "bg-blue-50 text-blue-700"}`}>
            ⏱ {formatDueRel(task.due_date)}
          </span>
        )}
        {task.source_message_id && (
          <span className="text-[10px] text-[#0EA5E9]" title="Has source message">↩</span>
        )}
        {task.linked_pr_url && (
          <span className="inline-flex items-center gap-0.5 rounded-full bg-purple-50 px-1.5 py-0.5 text-[10px] text-purple-700" title={task.linked_pr_url}>
            <GitPullRequest size={9} /> PR
          </span>
        )}
        {blocked && (
          <span className="inline-flex items-center gap-0.5 rounded-full bg-amber-50 px-1.5 py-0.5 text-[10px] text-amber-800" title={blockers.map((b) => b.title).join("\n")}>
            <Lock size={9} /> blocked × {blockers.length}
          </span>
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

function joinList(items?: string[] | null): string {
  return (items ?? []).filter(Boolean).join("\n");
}
function splitList(value: string): string[] {
  return value.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
}

function TaskDetailDrawer({
  task,
  allTasks,
  projectId,
  onClose,
  onUpdate,
  onDelete,
  onOpenSource,
  onDispatched,
}: {
  task: ProjectTask;
  allTasks: ProjectTask[];
  projectId: string;
  onClose: () => void;
  onUpdate: (taskId: string, patch: UpdateTaskInput) => Promise<ProjectTask>;
  onDelete: (t: ProjectTask) => void | Promise<void>;
  onOpenSource?: (conversationId: string, messageId: string) => void;
  onDispatched?: (conversationId: string, prompt: string) => void;
}) {
  const t = useT();
  const [form, setForm] = useState({
    title: task.title,
    why: task.why ?? "",
    acceptance_criteria: task.acceptance_criteria ?? "",
    ac_tests: joinList(task.acceptance_criteria_v2?.tests),
    ac_commands: joinList(task.acceptance_criteria_v2?.commands),
    ac_diff_hints: joinList(task.acceptance_criteria_v2?.diff_hints),
    ac_behavior: joinList(task.acceptance_criteria_v2?.behavior),
    test_plan: task.test_plan ?? "",
    rollback_plan: task.rollback_plan ?? "",
    definition_of_done: task.definition_of_done ?? "",
    estimated_effort: task.estimated_effort ?? "",
    affected_files: (task.affected_files ?? []).join(", "),
    priority: task.priority,
    status: task.status,
    assignee: task.assignee ?? "",
    due_date: task.due_date ?? "",
    labels: (task.labels ?? []).join(", "),
    linked_pr_url: task.linked_pr_url ?? "",
    linked_commit_sha: task.linked_commit_sha ?? "",
    depends_on: task.depends_on ?? [],
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [history, setHistory] = useState<TaskStatusEvent[] | null>(null);
  const [showHistory, setShowHistory] = useState(false);
  const [attempts, setAttempts] = useState<TaskAttempt[] | null>(null);
  const [showAttempts, setShowAttempts] = useState(false);
  const [dispatchMode, setDispatchMode] = useState<string>("openclaw");
  const [dispatching, setDispatching] = useState(false);

  const blockers = useMemo(() => findBlockers(task, allTasks), [task, allTasks]);
  const dependencyOptions = useMemo(
    () => allTasks.filter((other) => other.id !== task.id),
    [allTasks, task.id],
  );

  const dirty = useMemo(() => {
    const filesArr = form.affected_files.split(/[\s,]+/).map((f) => f.trim()).filter(Boolean);
    const labelsArr = form.labels.split(/[\s,]+/).map((l) => l.trim()).filter(Boolean);
    const currentFiles = task.affected_files ?? [];
    const currentLabels = task.labels ?? [];
    const currentDeps = task.depends_on ?? [];
    const filesChanged = filesArr.length !== currentFiles.length || filesArr.some((f, i) => f !== currentFiles[i]);
    const labelsChanged = labelsArr.length !== currentLabels.length || labelsArr.some((l, i) => l !== currentLabels[i]);
    const depsChanged = form.depends_on.length !== currentDeps.length ||
      form.depends_on.some((id, i) => id !== currentDeps[i]);
    const acV2 = task.acceptance_criteria_v2 ?? {};
    const acV2Changed = (
      form.ac_tests !== joinList(acV2.tests) ||
      form.ac_commands !== joinList(acV2.commands) ||
      form.ac_diff_hints !== joinList(acV2.diff_hints) ||
      form.ac_behavior !== joinList(acV2.behavior)
    );
    return (
      form.title !== task.title ||
      form.why !== (task.why ?? "") ||
      form.acceptance_criteria !== (task.acceptance_criteria ?? "") ||
      form.test_plan !== (task.test_plan ?? "") ||
      form.rollback_plan !== (task.rollback_plan ?? "") ||
      form.definition_of_done !== (task.definition_of_done ?? "") ||
      form.estimated_effort !== (task.estimated_effort ?? "") ||
      form.priority !== task.priority ||
      form.status !== task.status ||
      form.assignee !== (task.assignee ?? "") ||
      form.due_date !== (task.due_date ?? "") ||
      form.linked_pr_url !== (task.linked_pr_url ?? "") ||
      form.linked_commit_sha !== (task.linked_commit_sha ?? "") ||
      filesChanged || labelsChanged || depsChanged || acV2Changed
    );
  }, [form, task]);

  async function loadHistory() {
    if (history !== null) { setShowHistory(true); return; }
    try {
      const events = await tasksApi.history(projectId, task.id);
      setHistory(events);
      setShowHistory(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load history");
    }
  }

  async function loadAttempts() {
    if (attempts !== null) { setShowAttempts(true); return; }
    try {
      const list = await tasksApi.attempts(projectId, task.id);
      setAttempts(list);
      setShowAttempts(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load attempts");
    }
  }

  async function dispatch() {
    if (!onDispatched || dispatching) return;
    setDispatching(true);
    setError("");
    try {
      const result = await tasksApi.dispatch(projectId, task.id, { mode: dispatchMode });
      // Refresh attempts list (now includes the new one).
      try {
        const list = await tasksApi.attempts(projectId, task.id);
        setAttempts(list);
      } catch { /* best-effort */ }
      onDispatched(result.conversation_id, result.prompt);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Dispatch failed");
    } finally {
      setDispatching(false);
    }
  }

  function toggleDependency(id: string) {
    setForm((prev) => {
      const has = prev.depends_on.includes(id);
      return { ...prev, depends_on: has ? prev.depends_on.filter((d) => d !== id) : [...prev.depends_on, id] };
    });
  }

  async function save() {
    if (!form.title.trim()) {
      setError("Title required");
      return;
    }
    setSaving(true);
    setError("");
    try {
      const filesArr = form.affected_files.split(/[\s,]+/).map((f) => f.trim()).filter(Boolean);
      const labelsArr = form.labels.split(/[\s,]+/).map((l) => l.trim()).filter(Boolean);
      const acV2: AcceptanceCriteriaV2 = {
        tests: splitList(form.ac_tests),
        commands: splitList(form.ac_commands),
        diff_hints: splitList(form.ac_diff_hints),
        behavior: splitList(form.ac_behavior),
      };
      const acV2HasContent = (acV2.tests?.length ?? 0) + (acV2.commands?.length ?? 0)
        + (acV2.diff_hints?.length ?? 0) + (acV2.behavior?.length ?? 0) > 0;
      const patch: UpdateTaskInput = {
        title: form.title.trim(),
        why: form.why.trim(),
        acceptance_criteria: form.acceptance_criteria.trim(),
        acceptance_criteria_v2: acV2HasContent ? acV2 : null,
        test_plan: form.test_plan.trim(),
        rollback_plan: form.rollback_plan.trim(),
        definition_of_done: form.definition_of_done.trim(),
        estimated_effort: form.estimated_effort.trim(),
        affected_files: filesArr,
        priority: form.priority,
        status: form.status,
        assignee: form.assignee.trim(),
        labels: labelsArr,
        linked_pr_url: form.linked_pr_url.trim(),
        linked_commit_sha: form.linked_commit_sha.trim(),
        depends_on: form.depends_on,
      };
      // Send due_date only when set; null clears it.
      if (form.due_date) patch.due_date = form.due_date;
      else if (task.due_date) patch.due_date = null;
      await onUpdate(task.id, patch);
      // If status changed, refresh history so user sees the new entry.
      if (form.status !== task.status) {
        try {
          const events = await tasksApi.history(projectId, task.id);
          setHistory(events);
        } catch { /* best-effort */ }
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Save failed");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 z-40 flex justify-end bg-black/30" onClick={onClose}>
      <div
        className="h-full w-full max-w-[640px] overflow-y-auto bg-white shadow-2xl"
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

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.assignee")}</label>
              <input
                value={form.assignee}
                onChange={(e) => setForm({ ...form, assignee: e.target.value })}
                placeholder={t("roadmap.assigneeHint")}
                className="mt-1 h-9 w-full rounded-md border border-[#E2E8F0] px-2 text-sm"
              />
            </div>
            <div>
              <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.dueDate")}</label>
              <input
                type="date"
                value={form.due_date ?? ""}
                onChange={(e) => setForm({ ...form, due_date: e.target.value })}
                className="mt-1 h-9 w-full rounded-md border border-[#E2E8F0] px-2 text-sm"
              />
            </div>
          </div>

          <div>
            <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.labels")}</label>
            <input
              value={form.labels}
              onChange={(e) => setForm({ ...form, labels: e.target.value })}
              placeholder={t("roadmap.labelsHint")}
              className="mt-1 h-9 w-full rounded-md border border-[#E2E8F0] px-2 text-sm"
            />
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
              rows={3}
              className="mt-1 w-full rounded-md border border-[#E2E8F0] px-3 py-2 text-sm leading-6"
              placeholder={t("roadmap.acHint")}
            />
          </div>

          <div className="rounded-md border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-3 space-y-3">
            <div className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#475569]">
              {t("roadmap.acStructured")}
              <span className="ml-2 font-normal normal-case text-[10px] text-[#94A3B8]">{t("roadmap.acStructuredHint")}</span>
            </div>
            <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
              <div>
                <label className="text-[10px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.acTests")}</label>
                <textarea
                  value={form.ac_tests}
                  onChange={(e) => setForm({ ...form, ac_tests: e.target.value })}
                  rows={3}
                  className="mt-1 w-full rounded-md border border-[#E2E8F0] bg-white px-2 py-1.5 text-xs font-mono leading-5"
                  placeholder="cargo test --package backend api::tasks&#10;npm test"
                />
              </div>
              <div>
                <label className="text-[10px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.acCommands")}</label>
                <textarea
                  value={form.ac_commands}
                  onChange={(e) => setForm({ ...form, ac_commands: e.target.value })}
                  rows={3}
                  className="mt-1 w-full rounded-md border border-[#E2E8F0] bg-white px-2 py-1.5 text-xs font-mono leading-5"
                  placeholder="cd backend && cargo check&#10;cd web && npm run build"
                />
              </div>
              <div>
                <label className="text-[10px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.acDiff")}</label>
                <textarea
                  value={form.ac_diff_hints}
                  onChange={(e) => setForm({ ...form, ac_diff_hints: e.target.value })}
                  rows={3}
                  className="mt-1 w-full rounded-md border border-[#E2E8F0] bg-white px-2 py-1.5 text-xs leading-5"
                  placeholder={t("roadmap.acDiffHint")}
                />
              </div>
              <div>
                <label className="text-[10px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.acBehavior")}</label>
                <textarea
                  value={form.ac_behavior}
                  onChange={(e) => setForm({ ...form, ac_behavior: e.target.value })}
                  rows={3}
                  className="mt-1 w-full rounded-md border border-[#E2E8F0] bg-white px-2 py-1.5 text-xs leading-5"
                  placeholder={t("roadmap.acBehaviorHint")}
                />
              </div>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            <div>
              <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.testPlan")}</label>
              <textarea
                value={form.test_plan}
                onChange={(e) => setForm({ ...form, test_plan: e.target.value })}
                rows={3}
                className="mt-1 w-full rounded-md border border-[#E2E8F0] px-3 py-2 text-sm leading-6"
                placeholder={t("roadmap.testPlanHint")}
              />
            </div>
            <div>
              <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.rollback")}</label>
              <textarea
                value={form.rollback_plan}
                onChange={(e) => setForm({ ...form, rollback_plan: e.target.value })}
                rows={3}
                className="mt-1 w-full rounded-md border border-[#E2E8F0] px-3 py-2 text-sm leading-6"
                placeholder={t("roadmap.rollbackHint")}
              />
            </div>
          </div>

          <div>
            <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{t("roadmap.dod")}</label>
            <textarea
              value={form.definition_of_done}
              onChange={(e) => setForm({ ...form, definition_of_done: e.target.value })}
              rows={3}
              className="mt-1 w-full rounded-md border border-[#E2E8F0] px-3 py-2 text-sm leading-6"
              placeholder={t("roadmap.dodHint")}
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

          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            <div>
              <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">
                <GitPullRequest size={11} className="inline" /> {t("roadmap.prUrl")}
              </label>
              <input
                value={form.linked_pr_url}
                onChange={(e) => setForm({ ...form, linked_pr_url: e.target.value })}
                placeholder="https://github.com/owner/repo/pull/123"
                className="mt-1 h-9 w-full rounded-md border border-[#E2E8F0] px-2 text-xs font-mono"
              />
              {form.linked_pr_url && (
                <a href={form.linked_pr_url} target="_blank" rel="noreferrer" className="mt-1 inline-flex items-center gap-1 text-[10px] text-[#0050A0] hover:underline">
                  <ExternalLink size={9} /> {t("roadmap.openLink")}
                </a>
              )}
            </div>
            <div>
              <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">
                <GitBranch size={11} className="inline" /> {t("roadmap.commitSha")}
              </label>
              <input
                value={form.linked_commit_sha}
                onChange={(e) => setForm({ ...form, linked_commit_sha: e.target.value })}
                placeholder="abcd1234"
                className="mt-1 h-9 w-full rounded-md border border-[#E2E8F0] px-2 text-xs font-mono"
              />
            </div>
          </div>

          <div className="rounded-md border border-[#E2E8F0] bg-white">
            <div className="flex items-center justify-between gap-2 border-b border-[#E2E8F0] px-3 py-2">
              <div className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#475569]">
                <Lock size={11} className="inline" /> {t("roadmap.dependencies")}
                <span className="ml-2 text-[10px] font-normal text-[#94A3B8]">
                  {form.depends_on.length} {t("roadmap.depsSelected")}
                </span>
              </div>
              {blockers.length > 0 && (
                <span className="rounded-full bg-amber-50 px-2 py-0.5 text-[10px] text-amber-800">
                  {t("roadmap.blockedBy")} {blockers.length}
                </span>
              )}
            </div>
            <div className="max-h-40 overflow-auto px-3 py-2 space-y-1">
              {dependencyOptions.length === 0 ? (
                <div className="text-[11px] text-[#94A3B8]">{t("roadmap.depsEmpty")}</div>
              ) : (
                dependencyOptions.map((other) => {
                  const checked = form.depends_on.includes(other.id);
                  const closed = other.status === "done" || other.status === "cancelled";
                  return (
                    <label key={other.id} className="flex cursor-pointer items-center gap-2 rounded-md px-1 py-1 hover:bg-[#F8FAFC]">
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={() => toggleDependency(other.id)}
                        className="h-3 w-3"
                      />
                      <span className={`flex-1 truncate text-xs ${closed ? "text-[#94A3B8] line-through" : "text-[#1A1A2E]"}`}>
                        {other.title}
                      </span>
                      <span className={`text-[10px] px-1.5 py-0.5 rounded-full ${PRIORITY_BADGE[other.priority]}`}>{other.priority}</span>
                      <span className="text-[10px] text-[#94A3B8] uppercase">{other.status}</span>
                    </label>
                  );
                })
              )}
            </div>
          </div>

          {/* Send to Agent */}
          {onDispatched && (
            <div className="rounded-md border border-[#0050A0]/20 bg-[#EFF6FF] px-3 py-3">
              <div className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#0050A0]">
                <Send size={11} className="inline" /> {t("roadmap.sendToAgent")}
              </div>
              <p className="mt-1 text-[11px] text-[#475569]">{t("roadmap.sendToAgentHint")}</p>
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <select
                  value={dispatchMode}
                  onChange={(e) => setDispatchMode(e.target.value)}
                  className="h-8 rounded-md border border-[#E2E8F0] bg-white px-2 text-xs"
                >
                  <option value="openclaw">OpenClaw</option>
                  <option value="hermes">Hermes</option>
                  <option value="debate">Debate</option>
                </select>
                <button
                  type="button"
                  onClick={() => void dispatch()}
                  disabled={dispatching || dirty}
                  className="inline-flex items-center gap-1 rounded-md bg-[#0050A0] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#003B7A] disabled:bg-[#94A3B8]"
                  title={dirty ? t("roadmap.saveBeforeDispatch") : ""}
                >
                  <Send size={11} /> {dispatching ? t("common.loading") : t("roadmap.dispatchBtn")}
                </button>
                {dirty && <span className="text-[11px] text-[#B45309]">{t("roadmap.saveBeforeDispatch")}</span>}
              </div>
            </div>
          )}

          {/* Past attempts */}
          <div className="rounded-md border border-[#E2E8F0] bg-white">
            <button
              type="button"
              onClick={() => { if (!showAttempts) void loadAttempts(); else setShowAttempts(false); }}
              className="flex w-full items-center justify-between px-3 py-2 text-xs font-semibold text-[#475569] hover:bg-[#F8FAFC]"
            >
              <span className="inline-flex items-center gap-2">
                <History size={12} /> {t("roadmap.attempts")}
              </span>
              <span className="text-[#94A3B8]">{showAttempts ? "−" : "+"}</span>
            </button>
            {showAttempts && (
              <div className="px-3 py-2">
                {attempts === null ? (
                  <div className="text-xs text-[#94A3B8]">{t("common.loading")}</div>
                ) : attempts.length === 0 ? (
                  <div className="text-xs text-[#94A3B8]">{t("roadmap.attemptsEmpty")}</div>
                ) : (
                  <ul className="space-y-1 text-xs">
                    {attempts.map((a) => (
                      <li key={a.id} className="flex items-start gap-2">
                        <span className="font-mono text-[10px] text-[#94A3B8]">
                          {new Date(a.created_at).toLocaleString()}
                        </span>
                        <span className="text-[#475569]">
                          <span className="rounded-full bg-slate-100 px-1.5 py-0.5 text-[10px]">{a.mode}</span>
                          <span className="ml-1 text-[10px] uppercase text-[#94A3B8]">{a.status}</span>
                          {a.dispatched_by_name && <span className="ml-1 text-[#94A3B8]">by {a.dispatched_by_name}</span>}
                        </span>
                        {onOpenSource && (
                          <button
                            type="button"
                            onClick={() => onOpenSource(a.conversation_id, "")}
                            className="ml-auto inline-flex items-center gap-0.5 text-[10px] text-[#0050A0] hover:underline"
                          >
                            <ExternalLink size={9} /> {t("roadmap.openConv")}
                          </button>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
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

          {/* Status history */}
          <div className="rounded-md border border-[#E2E8F0] bg-white">
            <button
              type="button"
              onClick={() => { if (!showHistory) void loadHistory(); else setShowHistory(false); }}
              className="flex w-full items-center justify-between px-3 py-2 text-xs font-semibold text-[#475569] hover:bg-[#F8FAFC]"
            >
              <span className="inline-flex items-center gap-2">
                <History size={12} /> {t("roadmap.history")}
              </span>
              <span className="text-[#94A3B8]">{showHistory ? "−" : "+"}</span>
            </button>
            {showHistory && (
              <div className="px-3 py-2">
                {history === null ? (
                  <div className="text-xs text-[#94A3B8]">{t("common.loading")}</div>
                ) : history.length === 0 ? (
                  <div className="text-xs text-[#94A3B8]">{t("roadmap.historyEmpty")}</div>
                ) : (
                  <ul className="space-y-1 text-xs">
                    {history.map((h) => (
                      <li key={h.id} className="flex items-start gap-2">
                        <span className="font-mono text-[10px] text-[#94A3B8]">
                          {new Date(h.changed_at).toLocaleString()}
                        </span>
                        <span className="text-[#475569]">
                          {h.from_status ? (
                            <><span className="text-[#94A3B8]">{h.from_status}</span> → <span className="font-medium text-[#1A1A2E]">{h.to_status}</span></>
                          ) : (
                            <span className="font-medium text-[#1A1A2E]">{h.to_status}</span>
                          )}
                          {h.changed_by_name && <span className="ml-1 text-[#94A3B8]">by {h.changed_by_name}</span>}
                          {h.note && <span className="ml-1 italic text-[#64748B]">— {h.note}</span>}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </div>

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
