"use client";
import { useCallback, useEffect, useMemo, useState } from "react";
import { CalendarRange, CheckSquare, Download, ExternalLink, Filter, GitBranch, GitPullRequest, History, Lock, MessageCircle, Plus, RefreshCcw, Search, Send, Square, Tag, Trash2, X } from "lucide-react";
import {
  sprints as sprintsApi,
  tasks as tasksApi,
  type AcceptanceCriteriaV2,
  type CreateSprintInput,
  type ProjectTask,
  type Sprint,
  type TaskAttempt,
  type TaskComment,
  type TaskPriority,
  type TaskStatus,
  type TaskStatusEvent,
  type UpdateTaskInput,
} from "@/lib/api";
import { useT } from "@/lib/i18n";
import { useRoadmapFiltersStore } from "@/lib/store";

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

function startOfDayMs(value: string | Date): number {
  const date = value instanceof Date ? new Date(value) : new Date(value);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

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
    sprint_id: "" as string,
  });
  const [hoverCol, setHoverCol] = useState<TaskStatus | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);

  // C1: bulk-selection model. selectedIds is a set of task ids; the
  // bulk-action bar appears whenever it's non-empty.
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const clearSelection = () => setSelectedIds(new Set());
  const toggleSelected = (id: string) => setSelectedIds((prev) => {
    const next = new Set(prev);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });

  // Filter / sort state — persisted to localStorage per project so
  // reloading or coming back later restores what the user had set
  // (C2). We mirror the persisted values into local state for
  // controlled inputs, and write back to the store on every change.
  const persistedFilters = useRoadmapFiltersStore((s) => s.get(projectId));
  const persistFilter = useRoadmapFiltersStore((s) => s.set);
  const resetPersistedFilters = useRoadmapFiltersStore((s) => s.reset);
  const [search, setSearchRaw] = useState(persistedFilters.search);
  const [filterPriority, setFilterPriorityRaw] = useState<TaskPriority | "all">(persistedFilters.priority as TaskPriority | "all");
  const [filterAssignee, setFilterAssigneeRaw] = useState<string>(persistedFilters.assignee);
  const [filterLabel, setFilterLabelRaw] = useState<string>(persistedFilters.label);
  const [filterOverdue, setFilterOverdueRaw] = useState(persistedFilters.overdue);
  const [filterSprint, setFilterSprintRaw] = useState<string>(persistedFilters.sprint);
  const [sortKey, setSortKeyRaw] = useState<SortKey>(persistedFilters.sort as SortKey);

  // Wrap setters so every change also writes to the store.
  const setSearch = (v: string) => { setSearchRaw(v); persistFilter(projectId, { search: v }); };
  const setFilterPriority = (v: TaskPriority | "all") => { setFilterPriorityRaw(v); persistFilter(projectId, { priority: v }); };
  const setFilterAssignee = (v: string) => { setFilterAssigneeRaw(v); persistFilter(projectId, { assignee: v }); };
  const setFilterLabel    = (v: string) => { setFilterLabelRaw(v);    persistFilter(projectId, { label: v }); };
  const setFilterOverdue  = (v: boolean) => { setFilterOverdueRaw(v); persistFilter(projectId, { overdue: v }); };
  const setFilterSprint   = (v: string) => { setFilterSprintRaw(v);   persistFilter(projectId, { sprint: v }); };
  const setSortKey        = (v: SortKey) => { setSortKeyRaw(v);       persistFilter(projectId, { sort: v }); };

  // Sprint state
  const [sprintList, setSprintList] = useState<Sprint[]>([]);
  const [showSprintMgr, setShowSprintMgr] = useState(false);

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
      const [taskList, sprintListNew] = await Promise.all([
        tasksApi.list(projectId),
        sprintsApi.list(projectId).catch(() => [] as Sprint[]),
      ]);
      setItems(taskList);
      setSprintList(sprintListNew);
    } catch (e) {
      setErr(e instanceof Error ? e.message : "Failed to load tasks");
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  const reloadSprints = useCallback(async () => {
    try {
      setSprintList(await sprintsApi.list(projectId));
    } catch { /* best-effort */ }
  }, [projectId]);

  useEffect(() => {
    const timer = window.setTimeout(() => { void refresh(); }, 0);
    return () => window.clearTimeout(timer);
  }, [refresh]);

  // C3: lightweight polling so a second user's edits show up without
  // a manual refresh. 30s interval, paused while the tab is hidden
  // (Page Visibility API) so background tabs don't burn requests. We
  // skip the refresh while the drawer is open to avoid yanking the
  // user's in-progress edit out from under them.
  useEffect(() => {
    if (typeof document === "undefined") return;
    let cancelled = false;
    const tick = () => {
      if (cancelled) return;
      if (document.hidden) return;
      if (activeId) return; // someone is editing — wait
      void refresh();
    };
    const handle = window.setInterval(tick, 30_000);
    return () => { cancelled = true; window.clearInterval(handle); };
  }, [refresh, activeId]);

  // ----- C4: export -----------------------------------------------------
  function downloadBlob(content: string, mime: string, filename: string) {
    const blob = new Blob([content], { type: mime });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }
  function csvField(value: unknown): string {
    if (value === null || value === undefined) return "";
    let s = Array.isArray(value) ? value.join("|") : String(value);
    // RFC 4180-ish: wrap in quotes if it contains a comma / quote / newline,
    // and double-up any embedded quotes.
    if (/[",\n\r]/.test(s)) s = `"${s.replace(/"/g, '""')}"`;
    return s;
  }
  function exportTasks(format: "csv" | "json") {
    // Export only the currently filtered set so what the user sees is
    // what they get. Falls back to all items if no filters applied.
    const rows = filteredItems.length > 0 ? filteredItems : items;
    const ts = new Date().toISOString().replace(/[:.]/g, "-");
    if (format === "json") {
      downloadBlob(JSON.stringify(rows, null, 2), "application/json", `roadmap-${projectId.slice(0, 8)}-${ts}.json`);
      return;
    }
    const headers = [
      "id", "title", "status", "priority", "assignee", "due_date",
      "labels", "sprint_name", "epic_name", "estimated_effort",
      "linked_pr_url", "linked_commit_sha", "why", "acceptance_criteria",
      "created_at", "updated_at",
    ];
    const lines = [headers.join(",")];
    for (const r of rows) {
      lines.push(headers.map((h) => csvField((r as unknown as Record<string, unknown>)[h])).join(","));
    }
    downloadBlob(lines.join("\n"), "text/csv", `roadmap-${projectId.slice(0, 8)}-${ts}.csv`);
  }

  // ----- C1: bulk operations -------------------------------------------
  async function bulkApply(patch: UpdateTaskInput) {
    if (selectedIds.size === 0) return;
    const ids = Array.from(selectedIds);
    // Fire updates in parallel; collect successful + failed counts.
    let ok = 0, fail = 0;
    await Promise.all(ids.map(async (id) => {
      try { await tasksApi.update(projectId, id, patch); ok += 1; }
      catch { fail += 1; }
    }));
    await refresh();
    clearSelection();
    if (fail > 0) {
      setErr(`${ok} updated, ${fail} failed`);
    }
  }
  async function bulkDelete() {
    if (selectedIds.size === 0) return;
    if (!confirm(`Delete ${selectedIds.size} task(s)?`)) return;
    const ids = Array.from(selectedIds);
    await Promise.all(ids.map((id) => tasksApi.delete(projectId, id).catch(() => undefined)));
    await refresh();
    clearSelection();
  }

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
      sprint_id: draft.sprint_id || undefined,
    });
    setItems((prev) => [created, ...prev]);
    setDraft({ title: "", why: "", priority: "medium", affected_files: "", acceptance_criteria: "", estimated_effort: "", assignee: "", due_date: "", labels: "", sprint_id: "" });
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

  const filteredItems = (() => {
    const today = startOfDayMs(new Date());
    const q = search.trim().toLowerCase();
    let arr = items.filter((t) => {
      if (filterPriority !== "all" && t.priority !== filterPriority) return false;
      if (filterAssignee !== "all" && (t.assignee ?? "") !== filterAssignee) return false;
      if (filterLabel !== "all" && !(t.labels ?? []).includes(filterLabel)) return false;
      if (filterSprint === "none") {
        if (t.sprint_id) return false;
      } else if (filterSprint !== "all") {
        if (t.sprint_id !== filterSprint) return false;
      }
      if (filterOverdue) {
        if (!t.due_date) return false;
        if (t.status === "done" || t.status === "cancelled") return false;
        const due = startOfDayMs(t.due_date);
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
  })();

  const activeTask = useMemo(() => items.find((t) => t.id === activeId) ?? null, [items, activeId]);

  // Sprint task counts are computed client-side from `items` so they stay
  // in sync whenever a task is dragged between status columns, has its
  // sprint changed, or is created/deleted — without an extra refetch of
  // /sprints. The backend SPRINT_SELECT also returns task_total/task_done
  // for the initial load, but we override here so drag-drop reflects
  // instantly in the filter dropdown and the sprint manager.
  const sprintsWithLiveCounts: Sprint[] = useMemo(() => {
    const counts = new Map<string, { total: number; done: number }>();
    for (const tk of items) {
      if (!tk.sprint_id) continue;
      const c = counts.get(tk.sprint_id) ?? { total: 0, done: 0 };
      c.total += 1;
      if (tk.status === "done") c.done += 1;
      counts.set(tk.sprint_id, c);
    }
    return sprintList.map((s) => {
      const c = counts.get(s.id);
      return c ? { ...s, task_total: c.total, task_done: c.done } : { ...s, task_total: 0, task_done: 0 };
    });
  }, [sprintList, items]);

  if (loading && items.length === 0) {
    return <div className="p-8 text-center text-[#94A3B8]">{t("common.loading")}</div>;
  }
  if (err) return <div className="p-8 text-center text-[#C8102E]">{err}</div>;

  const grouped: Record<TaskStatus, ProjectTask[]> = {
    todo: [], "in-progress": [], done: [], cancelled: [],
  };
  for (const tk of filteredItems) grouped[tk.status]?.push(tk);

  const filtersActive = filterPriority !== "all" || filterAssignee !== "all" || filterLabel !== "all" || filterSprint !== "all" || filterOverdue || search.trim().length > 0;

  return (
    <div className="p-6 space-y-4 overflow-auto">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-[#1A1A2E]">{t("roadmap.title")}</h2>
          <p className="text-xs text-[#94A3B8]">{t("roadmap.subtitle")}</p>
        </div>
        <div className="flex items-center gap-2">
          <button onClick={() => void refresh()} className="text-xs text-[#0050A0] hover:underline">{t("common.refresh")}</button>
          <button
            onClick={() => exportTasks("csv")}
            className="inline-flex items-center gap-1 rounded-md border border-[#E2E8F0] bg-white px-2 py-1 text-xs text-[#475569] hover:border-[#0050A0] hover:text-[#0050A0]"
            title={t("roadmap.exportCsv")}
          >
            <Download size={12} /> CSV
          </button>
          <button
            onClick={() => exportTasks("json")}
            className="inline-flex items-center gap-1 rounded-md border border-[#E2E8F0] bg-white px-2 py-1 text-xs text-[#475569] hover:border-[#0050A0] hover:text-[#0050A0]"
            title={t("roadmap.exportJson")}
          >
            <Download size={12} /> JSON
          </button>
          <button
            onClick={() => setShowNew((v) => !v)}
            className="flex items-center gap-1 px-3 py-1.5 rounded-md bg-[#0050A0] text-white text-xs font-medium hover:bg-[#003B7A]"
          >
            <Plus size={12} /> {t("roadmap.newTask")}
          </button>
        </div>
      </div>

      {selectedIds.size > 0 && (
        <BulkActionBar
          count={selectedIds.size}
          sprintList={sprintsWithLiveCounts}
          onClear={clearSelection}
          onSelectAll={() => setSelectedIds(new Set(filteredItems.map((tk) => tk.id)))}
          onBulkApply={bulkApply}
          onBulkDelete={() => void bulkDelete()}
        />
      )}

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
          <select
            value={filterSprint}
            onChange={(e) => setFilterSprint(e.target.value)}
            className="h-7 rounded-md border border-[#E2E8F0] bg-white px-2"
            title={t("roadmap.sprint")}
          >
            <option value="all">{t("roadmap.allSprints")}</option>
            <option value="none">{t("roadmap.sprintBacklog")}</option>
            {sprintsWithLiveCounts.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name} ({s.task_done}/{s.task_total})
              </option>
            ))}
          </select>
          <button
            onClick={() => setShowSprintMgr(true)}
            className="inline-flex items-center gap-1 rounded-md border border-[#E2E8F0] bg-white px-2 py-1 text-[#475569] hover:border-[#0050A0] hover:text-[#0050A0]"
            title={t("roadmap.manageSprints")}
          >
            <CalendarRange size={11} /> {t("roadmap.manageSprints")}
          </button>
          {filtersActive && (
            <button
              onClick={() => {
                // Use the raw setters (skip persist write per call) and
                // then nuke the persisted record in one go so localStorage
                // doesn't churn through six writes for a single click.
                setSearchRaw(""); setFilterPriorityRaw("all"); setFilterAssigneeRaw("all");
                setFilterLabelRaw("all"); setFilterOverdueRaw(false); setFilterSprintRaw("all");
                resetPersistedFilters(projectId);
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
            <label className="text-xs text-[#64748B]">{t("roadmap.sprint")}:</label>
            <select
              value={draft.sprint_id}
              onChange={(e) => setDraft({ ...draft, sprint_id: e.target.value })}
              className="h-8 px-2 text-xs rounded-md border border-[#E2E8F0] max-w-[160px]"
            >
              <option value="">{t("roadmap.sprintBacklog")}</option>
              {sprintsWithLiveCounts.filter((s) => s.status !== "closed").map((s) => (
                <option key={s.id} value={s.id}>{s.name}</option>
              ))}
            </select>
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
                    selected={selectedIds.has(tk.id)}
                    onToggleSelect={() => toggleSelected(tk.id)}
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
          sprintList={sprintsWithLiveCounts}
          projectId={projectId}
          onClose={() => setActiveId(null)}
          onUpdate={updateTask}
          onDelete={deleteTask}
          onOpenSource={onOpenSource}
          onDispatched={onDispatched}
        />
      )}

      {showSprintMgr && (
        <SprintManagerModal
          projectId={projectId}
          sprints={sprintsWithLiveCounts}
          onClose={() => setShowSprintMgr(false)}
          onChanged={() => void reloadSprints()}
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
  selected,
  onToggleSelect,
  onMove,
  onDelete,
  onDragStart,
  onOpen,
}: {
  task: ProjectTask;
  allTasks: ProjectTask[];
  selected: boolean;
  onToggleSelect: () => void;
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
      className={`bg-white border rounded-md p-3 group hover:border-[#0050A0] cursor-pointer ${
        selected ? "border-[#0050A0] ring-2 ring-[#0050A0]/30"
        : overdue ? "border-red-300"
        : "border-[#E2E8F0]"
      }`}
    >
      <div className="flex items-start justify-between gap-2">
        {/* Checkbox stops propagation so clicking it doesn't also open
            the drawer; clicking the card itself still opens the drawer. */}
        <button
          type="button"
          onClick={(e) => { e.stopPropagation(); onToggleSelect(); }}
          className="mt-0.5 flex-shrink-0 text-[#94A3B8] hover:text-[#0050A0]"
          aria-label={selected ? "Deselect" : "Select"}
        >
          {selected ? <CheckSquare size={13} className="text-[#0050A0]" /> : <Square size={13} />}
        </button>
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
        {task.sprint_name && (
          <span className="inline-flex items-center gap-0.5 rounded-full bg-indigo-50 px-1.5 py-0.5 text-[10px] text-indigo-700" title={task.sprint_name}>
            <CalendarRange size={9} /> {task.sprint_name}
          </span>
        )}
        {!!task.comment_count && task.comment_count > 0 && (
          <span className="inline-flex items-center gap-0.5 rounded-full bg-sky-50 px-1.5 py-0.5 text-[10px] text-sky-700" title={`${task.comment_count} comments`}>
            <MessageCircle size={9} /> {task.comment_count}
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
  sprintList,
  projectId,
  onClose,
  onUpdate,
  onDelete,
  onOpenSource,
  onDispatched,
}: {
  task: ProjectTask;
  allTasks: ProjectTask[];
  sprintList: Sprint[];
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
    sprint_id: task.sprint_id ?? "",
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [history, setHistory] = useState<TaskStatusEvent[] | null>(null);
  const [showHistory, setShowHistory] = useState(false);
  const [attempts, setAttempts] = useState<TaskAttempt[] | null>(null);
  const [showAttempts, setShowAttempts] = useState(false);
  const [comments, setComments] = useState<TaskComment[] | null>(null);
  const [commentDraft, setCommentDraft] = useState("");
  const [submittingComment, setSubmittingComment] = useState(false);
  const [editingCommentId, setEditingCommentId] = useState<string | null>(null);
  const [editingCommentDraft, setEditingCommentDraft] = useState("");
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
      form.sprint_id !== (task.sprint_id ?? "") ||
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

  // Comments are loaded eagerly when the drawer opens so the count chip
  // in the header reflects reality without a click.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await tasksApi.comments(projectId, task.id);
        if (!cancelled) setComments(list);
      } catch {
        if (!cancelled) setComments([]);
      }
    })();
    return () => { cancelled = true; };
  }, [projectId, task.id]);

  async function submitComment() {
    const trimmed = commentDraft.trim();
    if (!trimmed || submittingComment) return;
    setSubmittingComment(true);
    setError("");
    try {
      const created = await tasksApi.addComment(projectId, task.id, trimmed);
      setComments((prev) => [...(prev ?? []), created]);
      setCommentDraft("");
    } catch (e) {
      setError(e instanceof Error ? e.message : "Comment failed");
    } finally {
      setSubmittingComment(false);
    }
  }

  function startEditComment(c: TaskComment) {
    setEditingCommentId(c.id);
    setEditingCommentDraft(c.content);
  }

  async function saveEditComment(c: TaskComment) {
    const trimmed = editingCommentDraft.trim();
    if (!trimmed || trimmed === c.content) {
      setEditingCommentId(null);
      return;
    }
    try {
      const updated = await tasksApi.updateComment(projectId, task.id, c.id, trimmed);
      setComments((prev) => (prev ?? []).map((x) => x.id === updated.id ? updated : x));
      setEditingCommentId(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Comment update failed");
    }
  }

  async function deleteComment(c: TaskComment) {
    if (!confirm("刪除這則留言？")) return;
    try {
      await tasksApi.deleteComment(projectId, task.id, c.id);
      setComments((prev) => (prev ?? []).filter((x) => x.id !== c.id));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Comment delete failed");
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
      // Always send the AC v2 object (even when all 4 lists are empty)
      // so the backend's COALESCE actually writes. Sending null silently
      // no-ops via COALESCE(NULL, old) = old, leaving the textareas blank
      // but DB unchanged, which kept the form dirty forever. The empty-
      // object case is treated as "no structured AC" by the prompt
      // builder, so falling through to the legacy free-text AC still works.
      const acV2: AcceptanceCriteriaV2 = {
        tests: splitList(form.ac_tests),
        commands: splitList(form.ac_commands),
        diff_hints: splitList(form.ac_diff_hints),
        behavior: splitList(form.ac_behavior),
      };
      const patch: UpdateTaskInput = {
        title: form.title.trim(),
        why: form.why.trim(),
        acceptance_criteria: form.acceptance_criteria.trim(),
        acceptance_criteria_v2: acV2,
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
      // Sprint: send the value (or explicit null) only when the user
      // actually changed it. Backend uses CASE WHEN $20 THEN $21 ELSE
      // sprint_id END, so omitting the field leaves the column alone.
      if (form.sprint_id !== (task.sprint_id ?? "")) {
        patch.sprint_id = form.sprint_id || null;
      }
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

          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
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
              <label className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">
                <CalendarRange size={11} className="inline" /> {t("roadmap.sprint")}
              </label>
              <select
                value={form.sprint_id}
                onChange={(e) => setForm({ ...form, sprint_id: e.target.value })}
                className="mt-1 h-9 w-full rounded-md border border-[#E2E8F0] px-2 text-sm"
              >
                <option value="">{t("roadmap.sprintBacklog")}</option>
                {sprintList.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}{s.status === "closed" ? " (closed)" : ""}
                  </option>
                ))}
              </select>
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
                    {attempts.map((a) => {
                      const isFailed = a.status === "failed" || a.status === "cancelled";
                      const statusTone =
                        a.status === "complete" ? "text-emerald-700"
                        : a.status === "running"  ? "text-amber-700"
                        : isFailed               ? "text-red-700"
                        : "text-[#94A3B8]";
                      return (
                        <li key={a.id} className="flex items-start gap-2">
                          <span className="font-mono text-[10px] text-[#94A3B8]">
                            {new Date(a.created_at).toLocaleString()}
                          </span>
                          <span className="text-[#475569]">
                            <span className="rounded-full bg-slate-100 px-1.5 py-0.5 text-[10px]">{a.mode}</span>
                            <span className={`ml-1 text-[10px] uppercase font-semibold ${statusTone}`}>{a.status}</span>
                            {a.dispatched_by_name && <span className="ml-1 text-[#94A3B8]">by {a.dispatched_by_name}</span>}
                          </span>
                          <div className="ml-auto flex gap-2">
                            {isFailed && onDispatched && !dirty && (
                              <button
                                type="button"
                                onClick={() => { setDispatchMode(a.mode); void dispatch(); }}
                                disabled={dispatching}
                                className="inline-flex items-center gap-0.5 text-[10px] text-[#0050A0] hover:underline disabled:opacity-50"
                                title={t("roadmap.retryDispatch")}
                              >
                                <RefreshCcw size={9} /> {t("roadmap.retry")}
                              </button>
                            )}
                            {onOpenSource && (
                              <button
                                type="button"
                                onClick={() => onOpenSource(a.conversation_id, "")}
                                className="inline-flex items-center gap-0.5 text-[10px] text-[#0050A0] hover:underline"
                              >
                                <ExternalLink size={9} /> {t("roadmap.openConv")}
                              </button>
                            )}
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </div>
            )}
          </div>

          {/* Comments thread */}
          <div className="rounded-md border border-[#E2E8F0] bg-white">
            <div className="flex items-center justify-between gap-2 border-b border-[#E2E8F0] px-3 py-2 text-xs font-semibold text-[#475569]">
              <span className="inline-flex items-center gap-2">
                <MessageCircle size={12} /> {t("roadmap.comments")}
              </span>
              <span className="text-[10px] font-normal text-[#94A3B8]">
                {comments === null ? "…" : `${comments.length} ${t("roadmap.commentCount")}`}
              </span>
            </div>
            <div className="px-3 py-2 space-y-2">
              {comments === null ? (
                <div className="text-xs text-[#94A3B8]">{t("common.loading")}</div>
              ) : comments.length === 0 ? (
                <div className="text-xs text-[#94A3B8]">{t("roadmap.commentsEmpty")}</div>
              ) : (
                <ul className="space-y-2">
                  {comments.map((c) => (
                    <li key={c.id} className="rounded-md border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-2">
                      <div className="flex items-baseline justify-between gap-2 text-[11px]">
                        <span className="font-semibold text-[#1A1A2E]">{c.author_name ?? t("roadmap.unknownAuthor")}</span>
                        <span className="text-[10px] text-[#94A3B8]">
                          {new Date(c.created_at).toLocaleString()}
                          {c.updated_at !== c.created_at && (
                            <span className="ml-1 italic">({t("roadmap.edited")})</span>
                          )}
                        </span>
                      </div>
                      {editingCommentId === c.id ? (
                        <div className="mt-1 space-y-1">
                          <textarea
                            value={editingCommentDraft}
                            onChange={(e) => setEditingCommentDraft(e.target.value)}
                            rows={3}
                            className="w-full rounded-md border border-[#E2E8F0] bg-white px-2 py-1.5 text-xs leading-5"
                          />
                          <div className="flex gap-1">
                            <button type="button" onClick={() => void saveEditComment(c)} className="rounded-md bg-[#0050A0] px-2 py-1 text-[11px] text-white hover:bg-[#003B7A]">
                              {t("common.save")}
                            </button>
                            <button type="button" onClick={() => setEditingCommentId(null)} className="rounded-md border border-[#E2E8F0] px-2 py-1 text-[11px] text-[#64748B] hover:bg-[#F1F5F9]">
                              {t("common.cancel")}
                            </button>
                          </div>
                        </div>
                      ) : (
                        <>
                          <div className="mt-1 whitespace-pre-wrap text-xs leading-5 text-[#1A1A2E]">{c.content}</div>
                          <div className="mt-1 flex gap-2 text-[10px]">
                            <button type="button" onClick={() => startEditComment(c)} className="text-[#0050A0] hover:underline">
                              {t("common.edit")}
                            </button>
                            <button type="button" onClick={() => void deleteComment(c)} className="text-[#C8102E] hover:underline">
                              {t("common.delete")}
                            </button>
                          </div>
                        </>
                      )}
                    </li>
                  ))}
                </ul>
              )}

              <div className="mt-2 space-y-1">
                <textarea
                  value={commentDraft}
                  onChange={(e) => setCommentDraft(e.target.value)}
                  placeholder={t("roadmap.commentPlaceholder")}
                  rows={2}
                  className="w-full rounded-md border border-[#E2E8F0] px-2 py-1.5 text-xs leading-5"
                />
                <button
                  type="button"
                  onClick={() => void submitComment()}
                  disabled={!commentDraft.trim() || submittingComment}
                  className="rounded-md bg-[#0050A0] px-3 py-1 text-[11px] font-medium text-white hover:bg-[#003B7A] disabled:bg-[#94A3B8]"
                >
                  {submittingComment ? t("common.loading") : t("roadmap.addComment")}
                </button>
              </div>
            </div>
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

function SprintManagerModal({
  projectId,
  sprints,
  onClose,
  onChanged,
}: {
  projectId: string;
  sprints: Sprint[];
  onClose: () => void;
  onChanged: () => void;
}) {
  const t = useT();
  const [draft, setDraft] = useState<CreateSprintInput>({ name: "", goal: "", start_date: "", end_date: "", status: "planned" });
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");

  async function create(e: React.FormEvent) {
    e.preventDefault();
    if (!draft.name.trim() || creating) return;
    setCreating(true);
    setError("");
    try {
      const payload: CreateSprintInput = {
        name: draft.name.trim(),
        status: draft.status,
      };
      if (draft.goal && draft.goal.trim()) payload.goal = draft.goal.trim();
      if (draft.start_date) payload.start_date = draft.start_date;
      if (draft.end_date) payload.end_date = draft.end_date;
      await sprintsApi.create(projectId, payload);
      setDraft({ name: "", goal: "", start_date: "", end_date: "", status: "planned" });
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Create failed");
    } finally {
      setCreating(false);
    }
  }

  async function updateStatus(s: Sprint, status: Sprint["status"]) {
    try {
      await sprintsApi.update(projectId, s.id, { status });
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Update failed");
    }
  }

  async function rename(s: Sprint) {
    const next = window.prompt(t("roadmap.sprintRenamePrompt"), s.name);
    if (next === null) return;
    const trimmed = next.trim();
    if (!trimmed || trimmed === s.name) return;
    try {
      await sprintsApi.update(projectId, s.id, { name: trimmed });
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Rename failed");
    }
  }

  async function removeSprint(s: Sprint) {
    if (!confirm(t("roadmap.sprintDeleteConfirm").replace("{name}", s.name))) return;
    try {
      await sprintsApi.delete(projectId, s.id);
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Delete failed");
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-6" onClick={onClose}>
      <div className="w-full max-w-2xl rounded-lg bg-white shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between border-b border-[#E2E8F0] px-5 py-3">
          <h3 className="text-sm font-semibold text-[#1A1A2E]">{t("roadmap.manageSprints")}</h3>
          <button onClick={onClose} className="rounded-md p-1 text-[#64748B] hover:bg-[#F1F5F9]"><X size={16} /></button>
        </div>

        <div className="max-h-[420px] overflow-y-auto px-5 py-4 space-y-3">
          {sprints.length === 0 ? (
            <div className="text-xs text-[#94A3B8]">{t("roadmap.sprintsEmpty")}</div>
          ) : (
            <ul className="space-y-2">
              {sprints.map((s) => (
                <li key={s.id} className="rounded-md border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-2">
                  <div className="flex items-center justify-between gap-2">
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-[#1A1A2E] truncate">{s.name}</span>
                        <span className={`text-[10px] px-1.5 py-0.5 rounded-full ${
                          s.status === "active" ? "bg-emerald-50 text-emerald-700"
                          : s.status === "closed" ? "bg-slate-100 text-slate-500"
                          : "bg-amber-50 text-amber-700"
                        }`}>{s.status}</span>
                      </div>
                      <div className="mt-0.5 text-[11px] text-[#64748B]">
                        {s.start_date ?? "—"} → {s.end_date ?? "—"} · {s.task_done}/{s.task_total} {t("roadmap.tasksDoneLabel")}
                      </div>
                      {s.goal && <div className="mt-1 text-xs text-[#475569] line-clamp-2">{s.goal}</div>}
                    </div>
                    <div className="flex shrink-0 gap-1">
                      <select
                        value={s.status}
                        onChange={(e) => void updateStatus(s, e.target.value as Sprint["status"])}
                        className="h-7 rounded-md border border-[#E2E8F0] bg-white px-1.5 text-[11px]"
                      >
                        <option value="planned">planned</option>
                        <option value="active">active</option>
                        <option value="closed">closed</option>
                      </select>
                      <button type="button" onClick={() => void rename(s)} className="rounded-md border border-[#E2E8F0] px-2 py-1 text-[11px] text-[#475569] hover:border-[#0050A0] hover:text-[#0050A0]">
                        {t("common.edit")}
                      </button>
                      <button type="button" onClick={() => void removeSprint(s)} className="rounded-md border border-red-200 px-2 py-1 text-[11px] text-red-700 hover:bg-red-50">
                        {t("common.delete")}
                      </button>
                    </div>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>

        <form onSubmit={create} className="border-t border-[#E2E8F0] px-5 py-3 space-y-2 bg-[#FBFCFE]">
          <div className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#475569]">{t("roadmap.sprintNew")}</div>
          <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
            <input
              required
              value={draft.name}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              placeholder={t("roadmap.sprintNamePlaceholder")}
              className="h-9 rounded-md border border-[#E2E8F0] px-2 text-sm"
            />
            <select
              value={draft.status}
              onChange={(e) => setDraft({ ...draft, status: e.target.value as Sprint["status"] })}
              className="h-9 rounded-md border border-[#E2E8F0] px-2 text-sm"
            >
              <option value="planned">planned</option>
              <option value="active">active</option>
              <option value="closed">closed</option>
            </select>
            <input
              type="date"
              value={draft.start_date ?? ""}
              onChange={(e) => setDraft({ ...draft, start_date: e.target.value })}
              className="h-9 rounded-md border border-[#E2E8F0] px-2 text-sm"
            />
            <input
              type="date"
              value={draft.end_date ?? ""}
              onChange={(e) => setDraft({ ...draft, end_date: e.target.value })}
              className="h-9 rounded-md border border-[#E2E8F0] px-2 text-sm"
            />
          </div>
          <textarea
            value={draft.goal ?? ""}
            onChange={(e) => setDraft({ ...draft, goal: e.target.value })}
            placeholder={t("roadmap.sprintGoalPlaceholder")}
            rows={2}
            className="w-full rounded-md border border-[#E2E8F0] px-2 py-1.5 text-sm"
          />
          {error && <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">{error}</div>}
          <div className="flex items-center justify-end gap-2">
            <button type="button" onClick={onClose} className="rounded-md px-3 py-1.5 text-xs text-[#64748B] hover:bg-[#F1F5F9]">{t("common.close")}</button>
            <button type="submit" disabled={!draft.name.trim() || creating} className="rounded-md bg-[#0050A0] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#003B7A] disabled:bg-[#94A3B8]">
              {creating ? t("common.loading") : t("roadmap.sprintCreate")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

function BulkActionBar({
  count,
  sprintList,
  onClear,
  onSelectAll,
  onBulkApply,
  onBulkDelete,
}: {
  count: number;
  sprintList: Sprint[];
  onClear: () => void;
  onSelectAll: () => void;
  onBulkApply: (patch: UpdateTaskInput) => void | Promise<void>;
  onBulkDelete: () => void;
}) {
  const t = useT();
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-lg border border-[#BFDBFE] bg-[#EFF6FF] px-3 py-2 text-xs">
      <span className="font-semibold text-[#0050A0]">
        {count} {t("roadmap.bulkSelected")}
      </span>
      <button onClick={onSelectAll} className="text-[#0050A0] hover:underline">{t("roadmap.bulkSelectAll")}</button>
      <button onClick={onClear} className="text-[#64748B] hover:text-[#1A1A2E]">{t("roadmap.bulkClear")}</button>
      <div className="ml-2 inline-flex items-center gap-1">
        <span className="text-[#64748B]">{t("roadmap.bulkSetStatus")}:</span>
        <select
          defaultValue=""
          onChange={(e) => {
            if (!e.target.value) return;
            void onBulkApply({ status: e.target.value as TaskStatus });
            e.target.value = "";
          }}
          className="h-7 rounded-md border border-[#E2E8F0] bg-white px-1.5"
        >
          <option value="">…</option>
          <option value="todo">{t("roadmap.colTodo")}</option>
          <option value="in-progress">{t("roadmap.colInProgress")}</option>
          <option value="done">{t("roadmap.colDone")}</option>
          <option value="cancelled">{t("roadmap.colCancelled")}</option>
        </select>
      </div>
      <div className="inline-flex items-center gap-1">
        <span className="text-[#64748B]">{t("roadmap.bulkSetPriority")}:</span>
        <select
          defaultValue=""
          onChange={(e) => {
            if (!e.target.value) return;
            void onBulkApply({ priority: e.target.value as TaskPriority });
            e.target.value = "";
          }}
          className="h-7 rounded-md border border-[#E2E8F0] bg-white px-1.5"
        >
          <option value="">…</option>
          <option value="low">{t("roadmap.priorityLow")}</option>
          <option value="medium">{t("roadmap.priorityMedium")}</option>
          <option value="high">{t("roadmap.priorityHigh")}</option>
          <option value="critical">{t("roadmap.priorityCritical")}</option>
        </select>
      </div>
      <div className="inline-flex items-center gap-1">
        <span className="text-[#64748B]">{t("roadmap.bulkSetSprint")}:</span>
        <select
          defaultValue=""
          onChange={(e) => {
            if (e.target.value === "") return;
            const v = e.target.value;
            void onBulkApply({ sprint_id: v === "_clear" ? null : v });
            e.target.value = "";
          }}
          className="h-7 rounded-md border border-[#E2E8F0] bg-white px-1.5 max-w-[140px]"
        >
          <option value="">…</option>
          <option value="_clear">{t("roadmap.sprintBacklog")}</option>
          {sprintList.filter((s) => s.status !== "closed").map((s) => (
            <option key={s.id} value={s.id}>{s.name}</option>
          ))}
        </select>
      </div>
      <div className="ml-auto">
        <button
          onClick={onBulkDelete}
          className="inline-flex items-center gap-1 rounded-md border border-red-200 bg-white px-2 py-1 text-red-700 hover:bg-red-50"
        >
          <Trash2 size={11} /> {t("common.delete")}
        </button>
      </div>
    </div>
  );
}
