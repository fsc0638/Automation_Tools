"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import Link from "next/link";
import { CalendarRange, ExternalLink, Filter, GitPullRequest, MessageCircle, Search } from "lucide-react";
import {
  epics as epicsApi,
  userViews,
  type Epic,
  type UserTask,
  type TaskStatus,
} from "@/lib/api";
import { useT } from "@/lib/i18n";

const STATUS_COLUMNS: TaskStatus[] = ["todo", "in-progress", "done", "cancelled"];

const PRIORITY_BADGE: Record<string, string> = {
  critical: "bg-red-100 text-red-700",
  high: "bg-orange-100 text-orange-700",
  medium: "bg-yellow-100 text-yellow-700",
  low: "bg-slate-100 text-slate-600",
};

/**
 * B1: Global Roadmap — cross-project board of every task the user
 * owns. Read-only board (no drag/drop here — per-project Roadmap is
 * still the canonical editor). Filters by project, epic, label,
 * assignee, and a free-text query.
 */
export default function GlobalRoadmapPage() {
  const t = useT();
  const [tasks, setTasks] = useState<UserTask[]>([]);
  const [epics, setEpics] = useState<Epic[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  const [filterProject, setFilterProject] = useState("");
  const [filterEpic, setFilterEpic] = useState("");
  const [filterAssignee, setFilterAssignee] = useState("");
  const [filterLabel, setFilterLabel] = useState("");
  const [search, setSearch] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const [taskList, epicList] = await Promise.all([
        userViews.tasks({
          projectId: filterProject || undefined,
          epicId: filterEpic || undefined,
          assignee: filterAssignee || undefined,
          label: filterLabel || undefined,
          q: search.trim() || undefined,
        }),
        epicsApi.list().catch(() => [] as Epic[]),
      ]);
      setTasks(taskList);
      setEpics(epicList);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load");
    } finally {
      setLoading(false);
    }
  }, [filterProject, filterEpic, filterAssignee, filterLabel, search]);

  useEffect(() => {
    // Small debounce so typing in the search box doesn't flood the
    // backend with one request per keystroke.
    const timer = window.setTimeout(() => { void refresh(); }, 250);
    return () => window.clearTimeout(timer);
  }, [refresh]);

  // Distinct values for filter dropdowns, derived from the current
  // result set. Deriving on the fly keeps the UI in sync with whatever
  // the user has scoped to.
  const projects = useMemo(() => {
    const map = new Map<string, string>();
    for (const t of tasks) map.set(t.project_id, t.project_name);
    return Array.from(map.entries()).sort((a, b) => a[1].localeCompare(b[1]));
  }, [tasks]);
  const assignees = useMemo(() => {
    const set = new Set<string>();
    for (const t of tasks) if (t.assignee) set.add(t.assignee);
    return Array.from(set).sort();
  }, [tasks]);
  const labels = useMemo(() => {
    const set = new Set<string>();
    for (const t of tasks) for (const l of t.labels) set.add(l);
    return Array.from(set).sort();
  }, [tasks]);

  const grouped: Record<TaskStatus, UserTask[]> = {
    todo: [], "in-progress": [], done: [], cancelled: [],
  };
  for (const tk of tasks) grouped[tk.status]?.push(tk);

  return (
    <div className="mx-auto flex max-w-7xl flex-col gap-4 p-6">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold text-[#1A1A2E]">{t("globalRoadmap.title")}</h1>
          <p className="text-xs text-[#94A3B8]">{t("globalRoadmap.subtitle")}</p>
        </div>
        <button onClick={() => void refresh()} className="text-xs text-[#0050A0] hover:underline">{t("common.refresh")}</button>
      </header>

      <div className="flex flex-wrap items-center gap-2 rounded-xl border border-[#E2E8F0] bg-white p-3 text-xs">
        <Filter size={13} className="text-[#94A3B8]" />
        <select value={filterProject} onChange={(e) => setFilterProject(e.target.value)} className="h-7 rounded-md border border-[#E2E8F0] bg-white px-2">
          <option value="">{t("globalRoadmap.allProjects")}</option>
          {projects.map(([id, name]) => <option key={id} value={id}>{name}</option>)}
        </select>
        <select value={filterEpic} onChange={(e) => setFilterEpic(e.target.value)} className="h-7 rounded-md border border-[#E2E8F0] bg-white px-2">
          <option value="">{t("globalRoadmap.allEpics")}</option>
          {epics.map((e) => <option key={e.id} value={e.id}>{e.name} ({e.task_done}/{e.task_total})</option>)}
        </select>
        <select value={filterAssignee} onChange={(e) => setFilterAssignee(e.target.value)} className="h-7 rounded-md border border-[#E2E8F0] bg-white px-2">
          <option value="">{t("globalRoadmap.allAssignees")}</option>
          {assignees.map((a) => <option key={a} value={a}>{a}</option>)}
        </select>
        <select value={filterLabel} onChange={(e) => setFilterLabel(e.target.value)} className="h-7 rounded-md border border-[#E2E8F0] bg-white px-2">
          <option value="">{t("globalRoadmap.allLabels")}</option>
          {labels.map((l) => <option key={l} value={l}>{l}</option>)}
        </select>
        <div className="ml-2 flex items-center gap-1 rounded-md border border-[#E2E8F0] bg-white px-2 py-0.5">
          <Search size={12} className="text-[#94A3B8]" />
          <input value={search} onChange={(e) => setSearch(e.target.value)} placeholder={t("globalRoadmap.searchPlaceholder")} className="w-44 bg-transparent text-xs outline-none" />
        </div>
        <div className="ml-auto text-[#94A3B8]">{tasks.length} {t("globalRoadmap.tasksLabel")}</div>
      </div>

      {error && <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">{error}</div>}

      {loading && tasks.length === 0 ? (
        <div className="p-8 text-center text-sm text-[#94A3B8]">{t("common.loading")}</div>
      ) : tasks.length === 0 ? (
        <div className="rounded-lg border border-dashed border-[#E2E8F0] p-12 text-center text-sm text-[#94A3B8]">{t("globalRoadmap.empty")}</div>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-4">
          {STATUS_COLUMNS.map((col) => (
            <div key={col} className="rounded-lg border border-transparent bg-[#F8FAFC] p-3">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-xs font-semibold uppercase tracking-wider text-[#64748B]">{t(`roadmap.col${col === "todo" ? "Todo" : col === "in-progress" ? "InProgress" : col === "done" ? "Done" : "Cancelled"}`)}</span>
                <span className="text-xs text-[#94A3B8]">{grouped[col].length}</span>
              </div>
              <div className="space-y-2">
                {grouped[col].map((tk) => (
                  <GlobalTaskCard key={tk.id} task={tk} />
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function GlobalTaskCard({ task }: { task: UserTask }) {
  return (
    <Link
      href={`/projects/${task.project_id}`}
      className="block rounded-md border border-[#E2E8F0] bg-white p-3 hover:border-[#0050A0]"
    >
      <div className="text-sm font-medium text-[#1A1A2E] leading-snug">{task.title}</div>
      <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[10px]">
        <span className="inline-flex items-center gap-0.5 rounded-full bg-slate-100 px-1.5 py-0.5 text-slate-600">
          <ExternalLink size={9} /> {task.project_name}
        </span>
        <span className={`rounded-full px-1.5 py-0.5 ${PRIORITY_BADGE[task.priority]}`}>{task.priority}</span>
        {task.assignee && (
          <span className="rounded-full bg-slate-100 px-1.5 py-0.5 text-slate-600">@{task.assignee}</span>
        )}
        {task.epic_name && (
          <span className="inline-flex items-center gap-0.5 rounded-full bg-fuchsia-50 px-1.5 py-0.5 text-fuchsia-700">
            🎯 {task.epic_name}
          </span>
        )}
        {task.sprint_name && (
          <span className="inline-flex items-center gap-0.5 rounded-full bg-indigo-50 px-1.5 py-0.5 text-indigo-700">
            <CalendarRange size={9} /> {task.sprint_name}
          </span>
        )}
        {task.linked_pr_url && (
          <span className="inline-flex items-center gap-0.5 rounded-full bg-purple-50 px-1.5 py-0.5 text-purple-700">
            <GitPullRequest size={9} /> PR
          </span>
        )}
        {task.comment_count > 0 && (
          <span className="inline-flex items-center gap-0.5 rounded-full bg-sky-50 px-1.5 py-0.5 text-sky-700">
            <MessageCircle size={9} /> {task.comment_count}
          </span>
        )}
      </div>
    </Link>
  );
}
