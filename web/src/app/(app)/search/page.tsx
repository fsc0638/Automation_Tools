"use client";

import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { MessageSquare, FileCode, ExternalLink, Search as SearchIcon } from "lucide-react";
import { userViews, type ConvHit, type FileHit } from "@/lib/api";
import { useT } from "@/lib/i18n";

type Mode = "conversations" | "code";

/**
 * B2 + B5: cross-project search. One query, two tabs:
 *   - "conversations" hits messages + conversation titles
 *   - "code" hits project_files.path (substring match)
 * Each hit links back to its project so context-switching is one click.
 */
export default function GlobalSearchPage() {
  const t = useT();
  const [q, setQ] = useState("");
  const [mode, setMode] = useState<Mode>("conversations");
  const [convHits, setConvHits] = useState<ConvHit[]>([]);
  const [fileHits, setFileHits] = useState<FileHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const search = useCallback(async () => {
    const term = q.trim();
    if (!term) {
      setConvHits([]);
      setFileHits([]);
      return;
    }
    setLoading(true);
    setError("");
    try {
      if (mode === "conversations") {
        setConvHits(await userViews.conversations(term));
      } else {
        setFileHits(await userViews.code(term));
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Search failed");
    } finally {
      setLoading(false);
    }
  }, [q, mode]);

  useEffect(() => {
    const timer = window.setTimeout(() => { void search(); }, 300);
    return () => window.clearTimeout(timer);
  }, [search]);

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-4 p-6">
      <header>
        <h1 className="text-2xl font-semibold text-[#1A1A2E]">{t("globalSearch.title")}</h1>
        <p className="text-xs text-[#94A3B8]">{t("globalSearch.subtitle")}</p>
      </header>

      <div className="rounded-xl border border-[#E2E8F0] bg-white p-3">
        <div className="flex items-center gap-2 rounded-md border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-2">
          <SearchIcon size={14} className="text-[#94A3B8]" />
          <input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={t("globalSearch.placeholder")}
            autoFocus
            className="w-full bg-transparent text-sm outline-none"
          />
        </div>
        <div className="mt-3 flex gap-1">
          <ModeTab active={mode === "conversations"} onClick={() => setMode("conversations")}>
            <MessageSquare size={12} className="mr-1 inline" /> {t("globalSearch.conversations")}
          </ModeTab>
          <ModeTab active={mode === "code"} onClick={() => setMode("code")}>
            <FileCode size={12} className="mr-1 inline" /> {t("globalSearch.code")}
          </ModeTab>
        </div>
      </div>

      {error && <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">{error}</div>}
      {loading && <div className="text-xs text-[#94A3B8]">{t("common.loading")}</div>}

      {mode === "conversations" ? (
        <ConversationResults hits={convHits} q={q.trim()} />
      ) : (
        <CodeResults hits={fileHits} q={q.trim()} />
      )}
    </div>
  );
}

function ModeTab({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      className={`rounded-md px-3 py-1 text-xs font-medium ${active ? "bg-[#EAF2FF] text-[#0050A0]" : "text-[#64748B] hover:bg-[#F8FAFC]"}`}
    >
      {children}
    </button>
  );
}

function ConversationResults({ hits, q }: { hits: ConvHit[]; q: string }) {
  const t = useT();
  if (!q) return <div className="text-sm text-[#94A3B8]">{t("globalSearch.startTyping")}</div>;
  if (hits.length === 0) return <div className="text-sm text-[#94A3B8]">{t("globalSearch.noResults")}</div>;
  return (
    <ul className="space-y-2">
      {hits.map((h) => (
        <li key={h.conversation_id} className="rounded-md border border-[#E2E8F0] bg-white p-3 hover:border-[#0050A0]">
          <div className="flex items-baseline justify-between gap-2">
            <Link href={`/projects/${h.project_id}`} className="text-sm font-medium text-[#1A1A2E] hover:text-[#0050A0] hover:underline">
              {h.title || "(untitled)"}
            </Link>
            <span className="text-[10px] text-[#94A3B8]">{new Date(h.updated_at).toLocaleString()}</span>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-2 text-[11px]">
            <span className="rounded-full bg-slate-100 px-1.5 py-0.5 text-slate-600">{h.project_name}</span>
            <span className="rounded-full bg-blue-50 px-1.5 py-0.5 text-blue-700">{h.mode}</span>
          </div>
          {h.snippet && (
            <p className="mt-2 line-clamp-2 text-xs leading-5 text-[#475569]">{highlight(h.snippet, q)}</p>
          )}
        </li>
      ))}
    </ul>
  );
}

function CodeResults({ hits, q }: { hits: FileHit[]; q: string }) {
  const t = useT();
  if (!q) return <div className="text-sm text-[#94A3B8]">{t("globalSearch.startTyping")}</div>;
  if (hits.length === 0) return <div className="text-sm text-[#94A3B8]">{t("globalSearch.noResults")}</div>;
  // Group by project so users see the cross-project distribution.
  const byProject = new Map<string, { name: string; hits: FileHit[] }>();
  for (const h of hits) {
    const e = byProject.get(h.project_id) ?? { name: h.project_name, hits: [] };
    e.hits.push(h);
    byProject.set(h.project_id, e);
  }
  return (
    <ul className="space-y-3">
      {Array.from(byProject.entries()).map(([pid, group]) => (
        <li key={pid} className="rounded-md border border-[#E2E8F0] bg-white">
          <div className="border-b border-[#E2E8F0] px-3 py-2 text-sm font-medium text-[#1A1A2E]">
            <Link href={`/projects/${pid}`} className="hover:text-[#0050A0] hover:underline">
              {group.name}
            </Link>
            <span className="ml-2 text-[10px] text-[#94A3B8]">{group.hits.length} {t("globalSearch.files")}</span>
          </div>
          <ul className="divide-y divide-[#E2E8F0]">
            {group.hits.map((h) => (
              <li key={pid + h.path} className="flex items-center justify-between gap-2 px-3 py-1.5 text-xs">
                <span className="truncate font-mono text-[#0050A0]">{highlight(h.path, q)}</span>
                <Link href={`/projects/${pid}`} className="flex-shrink-0 text-[#94A3B8] hover:text-[#0050A0]">
                  <ExternalLink size={11} />
                </Link>
              </li>
            ))}
          </ul>
        </li>
      ))}
    </ul>
  );
}

function highlight(text: string, q: string) {
  if (!q) return text;
  const idx = text.toLowerCase().indexOf(q.toLowerCase());
  if (idx < 0) return text;
  return (
    <>
      {text.slice(0, idx)}
      <mark className="bg-yellow-200 px-0.5">{text.slice(idx, idx + q.length)}</mark>
      {text.slice(idx + q.length)}
    </>
  );
}
