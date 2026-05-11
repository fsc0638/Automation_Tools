"use client";

import { type FormEvent, useCallback, useEffect, useState } from "react";
import { Target, Plus, Trash2 } from "lucide-react";
import { epics as epicsApi, type CreateEpicInput, type Epic } from "@/lib/api";
import { useT } from "@/lib/i18n";

const STATUS_BADGE: Record<Epic["status"], string> = {
  active:   "bg-emerald-50 text-emerald-700",
  planned:  "bg-amber-50 text-amber-700",
  done:     "bg-blue-50 text-blue-700",
  archived: "bg-slate-100 text-slate-500",
};

/**
 * B3: cross-project Epics — milestone buckets that group tasks across
 * any number of projects. CRUD interface; task assignment happens in
 * the per-task drawer in the Roadmap.
 */
export default function EpicsPage() {
  const t = useT();
  const [list, setList] = useState<Epic[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);
  const [draft, setDraft] = useState<CreateEpicInput>({ name: "", description: "", color: "#0050A0", status: "planned" });
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    try { setList(await epicsApi.list()); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => { void refresh(); }, 0);
    return () => window.clearTimeout(timer);
  }, [refresh]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!draft.name.trim()) return;
    setError("");
    try {
      await epicsApi.create({
        name: draft.name.trim(),
        description: draft.description?.trim() || undefined,
        color: draft.color,
        status: draft.status,
        target_date: draft.target_date,
      });
      setDraft({ name: "", description: "", color: "#0050A0", status: "planned" });
      setShowCreate(false);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Create failed");
    }
  }

  async function remove(e: Epic) {
    if (!confirm(t("epics.deleteConfirm").replace("{name}", e.name))) return;
    try {
      await epicsApi.delete(e.id);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Delete failed");
    }
  }

  async function updateStatus(e: Epic, status: Epic["status"]) {
    try {
      await epicsApi.update(e.id, { status });
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Update failed");
    }
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-4 p-6">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold text-[#1A1A2E]">
            <Target size={20} className="mr-2 inline" /> {t("epics.title")}
          </h1>
          <p className="text-xs text-[#94A3B8]">{t("epics.subtitle")}</p>
        </div>
        <button
          onClick={() => setShowCreate((v) => !v)}
          className="inline-flex items-center gap-1 rounded-md bg-[#0050A0] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#003B7A]"
        >
          <Plus size={12} /> {t("epics.newEpic")}
        </button>
      </header>

      {showCreate && (
        <form onSubmit={(e) => void submit(e)} className="space-y-3 rounded-lg border border-[#E2E8F0] bg-white p-4">
          <input
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            placeholder={t("epics.namePlaceholder")}
            className="h-10 w-full rounded-md border border-[#E2E8F0] px-3 text-sm"
            required
          />
          <textarea
            value={draft.description ?? ""}
            onChange={(e) => setDraft({ ...draft, description: e.target.value })}
            placeholder={t("epics.descPlaceholder")}
            rows={2}
            className="w-full rounded-md border border-[#E2E8F0] px-3 py-2 text-sm leading-6"
          />
          <div className="flex flex-wrap items-center gap-3 text-xs">
            <label>{t("epics.status")}:
              <select
                value={draft.status}
                onChange={(e) => setDraft({ ...draft, status: e.target.value as Epic["status"] })}
                className="ml-1 h-8 rounded-md border border-[#E2E8F0] px-2"
              >
                <option value="planned">planned</option>
                <option value="active">active</option>
                <option value="done">done</option>
                <option value="archived">archived</option>
              </select>
            </label>
            <label>{t("epics.targetDate")}:
              <input
                type="date"
                value={draft.target_date ?? ""}
                onChange={(e) => setDraft({ ...draft, target_date: e.target.value })}
                className="ml-1 h-8 rounded-md border border-[#E2E8F0] px-2"
              />
            </label>
            <label>{t("epics.color")}:
              <input
                type="color"
                value={draft.color ?? "#0050A0"}
                onChange={(e) => setDraft({ ...draft, color: e.target.value })}
                className="ml-1 h-8 w-12 rounded-md border border-[#E2E8F0] align-middle"
              />
            </label>
            <div className="flex-1" />
            <button type="button" onClick={() => setShowCreate(false)} className="text-[#64748B] hover:text-[#1A1A2E]">{t("common.cancel")}</button>
            <button type="submit" className="rounded-md bg-[#0050A0] px-3 py-1.5 font-medium text-white">{t("common.create")}</button>
          </div>
          {error && <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">{error}</div>}
        </form>
      )}

      {loading ? (
        <div className="p-8 text-center text-sm text-[#94A3B8]">{t("common.loading")}</div>
      ) : list.length === 0 ? (
        <div className="rounded-lg border border-dashed border-[#E2E8F0] p-12 text-center text-sm text-[#94A3B8]">{t("epics.empty")}</div>
      ) : (
        <ul className="space-y-2">
          {list.map((e) => (
            <li key={e.id} className="rounded-lg border border-[#E2E8F0] bg-white p-4 hover:border-[#0050A0]">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="h-3 w-3 rounded-full" style={{ background: e.color ?? "#0050A0" }} />
                    <h2 className="text-base font-semibold text-[#1A1A2E]">{e.name}</h2>
                    <span className={`rounded-full px-2 py-0.5 text-[10px] ${STATUS_BADGE[e.status]}`}>{e.status}</span>
                  </div>
                  {e.description && <p className="mt-1 text-xs text-[#64748B]">{e.description}</p>}
                  <div className="mt-2 flex flex-wrap items-center gap-3 text-[11px] text-[#94A3B8]">
                    <span>{e.task_done}/{e.task_total} {t("epics.tasksDone")}</span>
                    <span>{e.project_count} {t("epics.projects")}</span>
                    {e.target_date && <span>{t("epics.targetBy")} {e.target_date}</span>}
                  </div>
                </div>
                <div className="flex flex-shrink-0 gap-1">
                  <select
                    value={e.status}
                    onChange={(ev) => void updateStatus(e, ev.target.value as Epic["status"])}
                    className="h-7 rounded-md border border-[#E2E8F0] bg-white px-2 text-[11px]"
                  >
                    <option value="planned">planned</option>
                    <option value="active">active</option>
                    <option value="done">done</option>
                    <option value="archived">archived</option>
                  </select>
                  <button onClick={() => void remove(e)} className="rounded-md border border-red-200 px-2 py-1 text-[11px] text-red-700 hover:bg-red-50">
                    <Trash2 size={11} />
                  </button>
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
