"use client";

import { type FormEvent, useCallback, useEffect, useState } from "react";
import { CheckCircle2, NotebookPen, Plus, Pin, PinOff, Trash2, XCircle } from "lucide-react";
import {
  projectMemory,
  projects,
  sharedMemory,
  type CreateNoteInput,
  type Project,
  type ProjectMemoryCandidate,
  type SharedMemoryNote,
} from "@/lib/api";
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
  const [projectList, setProjectList] = useState<Project[]>([]);
  const [selectedProjectId, setSelectedProjectId] = useState("");
  const [candidates, setCandidates] = useState<ProjectMemoryCandidate[]>([]);
  const [candidateLoading, setCandidateLoading] = useState(false);
  const [reviewBusyId, setReviewBusyId] = useState("");
  const [candidateStatus, setCandidateStatus] = useState<"pending" | "approved" | "rejected">("pending");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const pendingCount = candidates.filter((candidate) => candidate.status === "pending").length;
  const visiblePendingIds = candidates.filter((candidate) => candidate.status === "pending").map((candidate) => candidate.id);
  const selectedCount = visiblePendingIds.filter((id) => selectedIds.has(id)).length;
  const allVisibleSelected = visiblePendingIds.length > 0 && visiblePendingIds.every((id) => selectedIds.has(id));

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

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void projects.list().then((items) => {
        setProjectList(items);
        setSelectedProjectId((current) => current || items[0]?.id || "");
      });
    }, 0);
    return () => window.clearTimeout(timer);
  }, []);

  const refreshCandidates = useCallback(async () => {
    if (!selectedProjectId) {
      setCandidates([]);
      setSelectedIds(new Set());
      return;
    }
    setCandidateLoading(true);
    try {
      setCandidates(await projectMemory.candidates(selectedProjectId, candidateStatus));
      setSelectedIds(new Set());
    } finally {
      setCandidateLoading(false);
    }
  }, [candidateStatus, selectedProjectId]);

  useEffect(() => {
    const timer = window.setTimeout(() => { void refreshCandidates(); }, 150);
    return () => window.clearTimeout(timer);
  }, [refreshCandidates]);

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

  function toggleCandidateSelection(candidateId: string) {
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(candidateId)) next.delete(candidateId);
      else next.add(candidateId);
      return next;
    });
  }

  function toggleSelectAll() {
    setSelectedIds((current) => {
      if (allVisibleSelected) return new Set();
      const next = new Set(current);
      visiblePendingIds.forEach((id) => next.add(id));
      return next;
    });
  }

  async function approveCandidate(candidate: ProjectMemoryCandidate) {
    if (!selectedProjectId) return;
    setReviewBusyId(candidate.id);
    try {
      await projectMemory.approveCandidate(selectedProjectId, candidate.id, "Approved from memory review page");
      await refreshCandidates();
    } finally {
      setReviewBusyId("");
    }
  }

  async function rejectCandidate(candidate: ProjectMemoryCandidate) {
    if (!selectedProjectId) return;
    if (!confirm(t("memory.rejectConfirm"))) return;
    const reviewNote = window.prompt("請輸入拒絕原因（可留空）", "") ?? "";
    setReviewBusyId(candidate.id);
    try {
      await projectMemory.rejectCandidate(selectedProjectId, candidate.id, reviewNote.trim() || undefined);
      await refreshCandidates();
    } finally {
      setReviewBusyId("");
    }
  }

  async function approveSelected() {
    if (!selectedProjectId || selectedCount === 0) return;
    setReviewBusyId("bulk-approve");
    try {
      await projectMemory.bulkApproveCandidates(selectedProjectId, visiblePendingIds.filter((id) => selectedIds.has(id)));
      await refreshCandidates();
    } finally {
      setReviewBusyId("");
    }
  }

  async function rejectSelected() {
    if (!selectedProjectId || selectedCount === 0) return;
    if (!confirm(`確定要拒絕已勾選的 ${selectedCount} 筆記憶候選？`)) return;
    setReviewBusyId("bulk-reject");
    try {
      await projectMemory.bulkRejectCandidates(selectedProjectId, visiblePendingIds.filter((id) => selectedIds.has(id)));
      await refreshCandidates();
    } finally {
      setReviewBusyId("");
    }
  }

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-4 p-6">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="type-page-title">
            <NotebookPen size={20} className="mr-2 inline" /> {t("memory.title")}
          </h1>
          <p className="type-meta mt-1">{t("memory.subtitle")}</p>
        </div>
        <button
          onClick={() => setShowCreate((v) => !v)}
          className="inline-flex items-center gap-1.5 rounded-xl bg-[#0050A0] px-3.5 py-2 text-[13px] font-medium tracking-[-0.01em] text-white shadow-[0_1px_2px_rgba(15,23,42,0.03)] transition hover:bg-[#003B7A]"
        >
          <Plus size={13} /> {t("memory.newNote")}
        </button>
      </header>

      <section className="rounded-2xl border border-[#E2E8F0] bg-white p-5 shadow-sm">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div className="type-card-title flex items-center gap-2">
              {t("memory.reviewTitle")}
              <span className="rounded-full bg-amber-50 px-2 py-0.5 text-[11px] font-semibold tracking-[0.04em] text-amber-700">
                {candidateStatus === "pending"
                  ? t("memory.pendingCount").replace("{n}", String(pendingCount))
                  : `目前顯示 ${candidateStatus === "approved" ? "已核准" : "已拒絕"}`}
              </span>
            </div>
            <p className="type-body-muted mt-1">{t("memory.reviewDesc")}</p>
          </div>
          <select
            value={selectedProjectId}
            onChange={(e) => setSelectedProjectId(e.target.value)}
            className="h-10 min-w-56 rounded-xl border border-[#D6DFEA] bg-white px-3 text-[13px] outline-none focus:border-[#0050A0] focus:ring-2 focus:ring-blue-100"
          >
            {projectList.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}
          </select>
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-2">
          {([
            ["pending", "待審核"],
            ["approved", "已核准"],
            ["rejected", "已拒絕"],
          ] as const).map(([status, label]) => (
            <button
              key={status}
              onClick={() => setCandidateStatus(status)}
              className={`rounded-full px-3 py-1.5 text-[12px] font-medium transition ${
                candidateStatus === status
                  ? "bg-[#0F172A] text-white"
                  : "border border-[#D6DFEA] bg-white text-[#475569] hover:border-[#94A3B8]"
              }`}
            >
              {label}
            </button>
          ))}
        </div>

        {candidateStatus === "pending" && candidates.length > 0 && (
          <div className="mt-4 flex flex-wrap items-center gap-2 rounded-2xl border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-2">
            <button
              onClick={toggleSelectAll}
              className="rounded-lg border border-[#D6DFEA] bg-white px-2.5 py-1.5 text-[12px] font-medium text-[#334155] transition hover:border-[#94A3B8]"
            >
              {allVisibleSelected ? "取消全選" : "全選"}
            </button>
            <button
              disabled={selectedCount === 0 || reviewBusyId === "bulk-approve"}
              onClick={() => void approveSelected()}
              className="inline-flex items-center gap-1.5 rounded-lg bg-emerald-600 px-3 py-1.5 text-[12px] font-medium text-white transition hover:bg-emerald-700 disabled:opacity-60"
            >
              <CheckCircle2 size={12} /> Approve selected ({selectedCount})
            </button>
            <button
              disabled={selectedCount === 0 || reviewBusyId === "bulk-reject"}
              onClick={() => void rejectSelected()}
              className="inline-flex items-center gap-1.5 rounded-lg border border-red-200 bg-white px-3 py-1.5 text-[12px] font-medium text-red-700 transition hover:bg-red-50 disabled:opacity-60"
            >
              <XCircle size={12} /> Reject selected
            </button>
          </div>
        )}

        {candidateLoading ? (
          <div className="type-meta mt-4 rounded-xl border border-dashed border-[#E2E8F0] p-6 text-center">{t("memory.loadingCandidates")}</div>
        ) : candidates.length === 0 ? (
          <div className="type-meta mt-4 rounded-xl border border-dashed border-[#E2E8F0] p-6 text-center">
            {candidateStatus === "pending" ? t("memory.noPendingCandidates") : `此專案目前沒有${candidateStatus === "approved" ? "已核准" : "已拒絕"}的記憶候選。`}
          </div>
        ) : (
          <div className="mt-4 space-y-3">
            {candidates.map((candidate) => (
              <article key={candidate.id} className="rounded-2xl border border-amber-200 bg-amber-50/35 p-4">
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="flex min-w-0 flex-1 gap-3">
                    {candidate.status === "pending" && (
                      <input
                        type="checkbox"
                        checked={selectedIds.has(candidate.id)}
                        onChange={() => toggleCandidateSelection(candidate.id)}
                        className="mt-1 h-4 w-4 rounded border-[#CBD5E1] text-[#0050A0]"
                      />
                    )}
                    <div>
                      <div className="type-overline text-amber-700">{t("memory.candidateLabel")}</div>
                      <div className="mt-1 text-[11px] text-[#94A3B8]">
                        {candidate.source_message_count} {t("memory.sourceMessagesSuffix")} · {new Date(candidate.created_at).toLocaleString()}
                      </div>
                      {candidate.reviewed_by && (
                        <div className="mt-1.5 text-[11px] text-[#94A3B8]">
                          Reviewed by {candidate.reviewed_by.slice(0, 8)}…
                          {candidate.reviewed_at ? ` · ${new Date(candidate.reviewed_at).toLocaleString()}` : ""}
                          {candidate.review_note ? ` · "${candidate.review_note}"` : ""}
                        </div>
                      )}
                    </div>
                  </div>
                  {candidate.status === "pending" && (
                    <div className="flex gap-2">
                      <button
                        disabled={reviewBusyId === candidate.id}
                        onClick={() => void approveCandidate(candidate)}
                        className="inline-flex items-center gap-1.5 rounded-xl bg-emerald-600 px-3.5 py-2 text-[13px] font-medium tracking-[-0.01em] text-white shadow-[0_1px_2px_rgba(15,23,42,0.03)] transition hover:bg-emerald-700 disabled:opacity-60"
                      >
                        <CheckCircle2 size={13} /> {t("memory.approve")}
                      </button>
                      <button
                        disabled={reviewBusyId === candidate.id}
                        onClick={() => void rejectCandidate(candidate)}
                        className="inline-flex items-center gap-1.5 rounded-xl border border-red-200 bg-white px-3.5 py-2 text-[13px] font-medium tracking-[-0.01em] text-red-700 transition hover:bg-red-50 disabled:opacity-60"
                      >
                        <XCircle size={13} /> {t("memory.reject")}
                      </button>
                    </div>
                  )}
                </div>
                <pre className="mt-3 max-h-72 overflow-auto whitespace-pre-wrap rounded-xl bg-white p-3 text-[13px] leading-6 text-[#334155]">
                  {candidate.proposed_content}
                </pre>
              </article>
            ))}
          </div>
        )}
      </section>

      <input
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder={t("memory.searchPlaceholder")}
        className="h-10 rounded-xl border border-[#D6DFEA] bg-white px-3.5 text-[14px] leading-6 outline-none focus:border-[#0050A0] focus:ring-2 focus:ring-blue-100"
      />

      {showCreate && (
        <form onSubmit={(e) => void submit(e)} className="space-y-3 rounded-2xl border border-[#E2E8F0] bg-white p-5 shadow-sm">
          <input
            value={draft.title}
            onChange={(e) => setDraft({ ...draft, title: e.target.value })}
            placeholder={t("memory.titlePlaceholder")}
            className="h-11 w-full rounded-xl border border-[#D6DFEA] px-3.5 text-[14px] leading-6 outline-none focus:border-[#0050A0] focus:ring-2 focus:ring-blue-100"
            required
          />
          <textarea
            value={draft.body}
            onChange={(e) => setDraft({ ...draft, body: e.target.value })}
            placeholder={t("memory.bodyPlaceholder")}
            rows={5}
            className="w-full rounded-xl border border-[#D6DFEA] px-3.5 py-2.5 text-[14px] leading-7 outline-none focus:border-[#0050A0] focus:ring-2 focus:ring-blue-100"
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
        <div className="type-meta rounded-2xl border border-dashed border-[#E2E8F0] p-12 text-center">{t("memory.empty")}</div>
      ) : (
        <ul className="space-y-2.5">
          {list.map((n) => (
            <li key={n.id} className="rounded-2xl border border-[#E2E8F0] bg-white p-4 shadow-sm">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline gap-2">
                    <h2 className="type-card-title">{n.title}</h2>
                    {n.pinned && <Pin size={12} className="text-amber-600" />}
                  </div>
                  <p className="mt-1 whitespace-pre-wrap text-[13px] leading-6 text-[#475569]">{n.body}</p>
                  <div className="mt-2 flex flex-wrap items-center gap-2 text-[11px]">
                    {n.tags.map((tag) => (
                      <span key={tag} className="rounded-full bg-slate-100 px-2 py-0.5 text-slate-600">{tag}</span>
                    ))}
                    <span className="rounded-full bg-blue-50 px-2 py-0.5 text-blue-700">
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
