"use client";

import { type FormEvent, useCallback, useEffect, useState } from "react";
import { NotebookPen, Plus, Pin, PinOff, Trash2 } from "lucide-react";
import { sharedMemory, type CreateNoteInput, type SharedMemoryNote } from "@/lib/api";
import { useT } from "@/lib/i18n";

/**
 * B6: Shared memory — pinned facts / decisions opt-in visible to one,
 * many, or all of the user's projects. Lightweight per-note CRUD.
 * Tag filter + global query.
 */
export default function MemoryPage() {
  const t = useT();
  const [list, setList] = useState<SharedMemoryNote[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [draft, setDraft] = useState<CreateNoteInput>({ title: "", body: "", tags: [], scope_projects: [], pinned: false });
  const [tagInput, setTagInput] = useState("");
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setList(await sharedMemory.list({ q: search.trim() || undefined }));
    } finally {
      setLoading(false);
    }
  }, [search]);

  useEffect(() => {
    const timer = window.setTimeout(() => { void refresh(); }, 250);
    return () => window.clearTimeout(timer);
  }, [refresh]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!draft.title.trim() || !draft.body.trim()) return;
    setError("");
    try {
      await sharedMemory.create({
        title: draft.title.trim(),
        body: draft.body.trim(),
        tags: (draft.tags ?? []).filter(Boolean),
        scope_projects: draft.scope_projects ?? [],
        pinned: draft.pinned,
      });
      setDraft({ title: "", body: "", tags: [], scope_projects: [], pinned: false });
      setShowCreate(false);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Create failed");
    }
  }

  async function togglePin(n: SharedMemoryNote) {
    await sharedMemory.update(n.id, { pinned: !n.pinned });
    await refresh();
  }

  async function remove(n: SharedMemoryNote) {
    if (!confirm(t("memory.deleteConfirm").replace("{title}", n.title))) return;
    await sharedMemory.delete(n.id);
    await refresh();
  }

  function addTagToDraft() {
    const v = tagInput.trim();
    if (!v) return;
    setDraft({ ...draft, tags: [...(draft.tags ?? []), v] });
    setTagInput("");
  }

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-4 p-6">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold text-[#1A1A2E]">
            <NotebookPen size={20} className="mr-2 inline" /> {t("memory.title")}
          </h1>
          <p className="text-xs text-[#94A3B8]">{t("memory.subtitle")}</p>
        </div>
        <button
          onClick={() => setShowCreate((v) => !v)}
          className="inline-flex items-center gap-1 rounded-md bg-[#0050A0] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#003B7A]"
        >
          <Plus size={12} /> {t("memory.newNote")}
        </button>
      </header>

      <input
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder={t("memory.searchPlaceholder")}
        className="h-9 rounded-md border border-[#E2E8F0] bg-white px-3 text-sm"
      />

      {showCreate && (
        <form onSubmit={(e) => void submit(e)} className="space-y-3 rounded-lg border border-[#E2E8F0] bg-white p-4">
          <input
            value={draft.title}
            onChange={(e) => setDraft({ ...draft, title: e.target.value })}
            placeholder={t("memory.titlePlaceholder")}
            className="h-10 w-full rounded-md border border-[#E2E8F0] px-3 text-sm"
            required
          />
          <textarea
            value={draft.body}
            onChange={(e) => setDraft({ ...draft, body: e.target.value })}
            placeholder={t("memory.bodyPlaceholder")}
            rows={5}
            className="w-full rounded-md border border-[#E2E8F0] px-3 py-2 text-sm leading-6"
            required
          />
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <input
              value={tagInput}
              onChange={(e) => setTagInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); addTagToDraft(); } }}
              placeholder={t("memory.tagPlaceholder")}
              className="h-8 rounded-md border border-[#E2E8F0] bg-white px-2"
            />
            <button type="button" onClick={addTagToDraft} className="rounded-md border border-[#E2E8F0] px-2 py-1 hover:border-[#0050A0]">
              {t("memory.addTag")}
            </button>
            {(draft.tags ?? []).map((tag, i) => (
              <span key={i} className="rounded-full bg-slate-100 px-2 py-0.5 text-[10px]">
                {tag}
                <button type="button" onClick={() => setDraft({ ...draft, tags: (draft.tags ?? []).filter((_, j) => j !== i) })} className="ml-1 text-slate-500 hover:text-red-700">×</button>
              </span>
            ))}
            <label className="ml-auto inline-flex items-center gap-1">
              <input type="checkbox" checked={draft.pinned ?? false} onChange={(e) => setDraft({ ...draft, pinned: e.target.checked })} className="h-3 w-3" />
              {t("memory.pinned")}
            </label>
          </div>
          {error && <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">{error}</div>}
          <div className="flex justify-end gap-2 text-xs">
            <button type="button" onClick={() => setShowCreate(false)} className="text-[#64748B] hover:text-[#1A1A2E]">{t("common.cancel")}</button>
            <button type="submit" className="rounded-md bg-[#0050A0] px-3 py-1.5 font-medium text-white">{t("common.create")}</button>
          </div>
        </form>
      )}

      {loading ? (
        <div className="p-8 text-center text-sm text-[#94A3B8]">{t("common.loading")}</div>
      ) : list.length === 0 ? (
        <div className="rounded-lg border border-dashed border-[#E2E8F0] p-12 text-center text-sm text-[#94A3B8]">{t("memory.empty")}</div>
      ) : (
        <ul className="space-y-2">
          {list.map((n) => (
            <li key={n.id} className="rounded-lg border border-[#E2E8F0] bg-white p-4">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline gap-2">
                    <h2 className="text-sm font-semibold text-[#1A1A2E]">{n.title}</h2>
                    {n.pinned && <Pin size={11} className="text-amber-600" />}
                  </div>
                  <p className="mt-1 whitespace-pre-wrap text-xs leading-5 text-[#475569]">{n.body}</p>
                  <div className="mt-2 flex flex-wrap items-center gap-2 text-[10px]">
                    {n.tags.map((tag) => (
                      <span key={tag} className="rounded-full bg-slate-100 px-1.5 py-0.5 text-slate-600">{tag}</span>
                    ))}
                    <span className="rounded-full bg-blue-50 px-1.5 py-0.5 text-blue-700">
                      {n.scope_projects.length === 0 ? t("memory.scopeAll") : t("memory.scopeN").replace("{n}", String(n.scope_projects.length))}
                    </span>
                    <span className="text-[#94A3B8]">{new Date(n.updated_at).toLocaleString()}</span>
                  </div>
                </div>
                <div className="flex flex-shrink-0 gap-1">
                  <button onClick={() => void togglePin(n)} className="rounded-md border border-[#E2E8F0] p-1.5 text-[#64748B] hover:border-amber-400 hover:text-amber-600">
                    {n.pinned ? <PinOff size={11} /> : <Pin size={11} />}
                  </button>
                  <button onClick={() => void remove(n)} className="rounded-md border border-red-200 p-1.5 text-red-700 hover:bg-red-50">
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
