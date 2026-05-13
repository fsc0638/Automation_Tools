"use client";

import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { BarChart3, ExternalLink } from "lucide-react";
import {
  ResponsiveContainer, BarChart, Bar, XAxis, YAxis, Tooltip, CartesianGrid,
  Cell, ComposedChart, Area, Line,
} from "recharts";
import { userViews, type DebateHealth, type UserUsage } from "@/lib/api";
import { useT } from "@/lib/i18n";

const PROJECT_COLORS = ["#0050A0", "#7C3AED", "#F59E0B", "#10B981", "#C8102E", "#0EA5E9", "#EC4899"];
const AGENT_COLORS: Record<string, string> = {
  openclaw: "#7C3AED",
  hermes: "#0050A0",
};
function colorForAgent(agent: string, fallbackIndex: number) {
  return AGENT_COLORS[agent.toLowerCase()] ?? PROJECT_COLORS[fallbackIndex % PROJECT_COLORS.length];
}

const WINDOW_OPTIONS = [
  { value: 7,   label: "7d" },
  { value: 30,  label: "30d" },
  { value: 60,  label: "60d" },
  { value: 90,  label: "90d" },
  { value: 180, label: "180d" },
];

/**
 * B4: Global Insights — cross-project aggregation of agent_usage_events.
 * Shows total cost / tokens / calls, per-project + per-agent breakdowns,
 * a daily trend, and a debate-health rollup. The lookback window is
 * user-selectable; backend clamps to [1, 365].
 */
export default function GlobalInsightsPage() {
  const t = useT();
  const [usage, setUsage] = useState<UserUsage | null>(null);
  const [health, setHealth] = useState<DebateHealth | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [days, setDays] = useState(30);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const [u, h] = await Promise.all([
        userViews.usage(days),
        userViews.debateHealth(days).catch(() => null),
      ]);
      setUsage(u);
      setHealth(h);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load");
    } finally {
      setLoading(false);
    }
  }, [days]);

  useEffect(() => {
    const timer = window.setTimeout(() => { void refresh(); }, 0);
    return () => window.clearTimeout(timer);
  }, [refresh]);

  if (loading && !usage) {
    return <div className="p-8 text-center text-sm text-[#94A3B8]">{t("common.loading")}</div>;
  }
  if (error) {
    return <div className="p-8 text-center text-sm text-[#C8102E]">{error}</div>;
  }
  if (!usage) return null;

  return (
    <div className="mx-auto flex max-w-7xl flex-col gap-6 p-6">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold text-[#1A1A2E]">
            <BarChart3 size={20} className="mr-2 inline" />
            {t("globalInsights.title")}
          </h1>
          <p className="text-xs text-[#94A3B8]">
            {t("globalInsights.subtitle")} · Last {usage.days} days
          </p>
        </div>
        <div className="flex items-center gap-3">
          {/* Window selector — server clamps days into [1, 365] so anything
              here is safe. Defaults to 30. */}
          <div className="flex items-center rounded-lg border border-[#E2E8F0] bg-white p-0.5 text-xs">
            {WINDOW_OPTIONS.map((opt) => (
              <button
                key={opt.value}
                type="button"
                onClick={() => setDays(opt.value)}
                className={
                  "rounded-md px-2.5 py-1 transition " +
                  (days === opt.value
                    ? "bg-[#0050A0] text-white"
                    : "text-[#475569] hover:bg-[#F1F5F9]")
                }
              >
                {opt.label}
              </button>
            ))}
          </div>
          <button onClick={() => void refresh()} className="text-xs text-[#0050A0] hover:underline">{t("common.refresh")}</button>
        </div>
      </header>

      <section className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Kpi label={t("globalInsights.totalCost")}   value={`$${usage.total_cost_usd.toFixed(2)}`} />
        <Kpi label={t("globalInsights.totalCalls")}  value={usage.total_calls.toLocaleString()} />
        <Kpi label={t("globalInsights.tokensIn")}    value={usage.total_tokens_in.toLocaleString()} />
        <Kpi label={t("globalInsights.tokensOut")}   value={usage.total_tokens_out.toLocaleString()} />
      </section>

      <div className="grid gap-4 lg:grid-cols-2">
        <section className="rounded-lg border border-[#E2E8F0] bg-white p-4">
          <div className="mb-3 text-sm font-semibold text-[#1A1A2E]">{t("globalInsights.byProject")}</div>
          {usage.by_project.length === 0 ? (
            <Empty />
          ) : (
            <ResponsiveContainer width="100%" height={220}>
              <BarChart data={usage.by_project}>
                <CartesianGrid strokeDasharray="3 3" stroke="#E2E8F0" />
                <XAxis dataKey="project_name" tick={{ fontSize: 11 }} />
                <YAxis tick={{ fontSize: 11 }} tickFormatter={(v) => `$${Number(v).toFixed(2)}`} />
                <Tooltip formatter={(v) => `$${Number(v).toFixed(4)}`} />
                <Bar dataKey="cost_usd" radius={[4, 4, 0, 0]}>
                  {usage.by_project.map((_, i) => (
                    <Cell key={i} fill={PROJECT_COLORS[i % PROJECT_COLORS.length]} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          )}
        </section>

        {/* DEFERRED 11: by-agent breakdown. OpenClaw vs Hermes vs any custom
            agent profiles registered under that user. */}
        <section className="rounded-lg border border-[#E2E8F0] bg-white p-4">
          <div className="mb-3 text-sm font-semibold text-[#1A1A2E]">{t("insights.byAgentTitle")}</div>
          {usage.by_agent.length === 0 ? (
            <Empty />
          ) : (
            <ResponsiveContainer width="100%" height={220}>
              <BarChart data={usage.by_agent}>
                <CartesianGrid strokeDasharray="3 3" stroke="#E2E8F0" />
                <XAxis dataKey="agent" tick={{ fontSize: 11 }} />
                <YAxis tick={{ fontSize: 11 }} tickFormatter={(v) => `$${Number(v).toFixed(2)}`} />
                <Tooltip formatter={(v) => `$${Number(v).toFixed(4)}`} />
                <Bar dataKey="cost_usd" radius={[4, 4, 0, 0]}>
                  {usage.by_agent.map((u, i) => (
                    <Cell key={u.agent} fill={colorForAgent(u.agent, i)} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          )}
        </section>
      </div>

      <section className="rounded-lg border border-[#E2E8F0] bg-white p-4">
        <div className="mb-3 text-sm font-semibold text-[#1A1A2E]">{t("globalInsights.dailyTrend")}</div>
        {usage.daily.length === 0 ? (
          <Empty />
        ) : (
          <ResponsiveContainer width="100%" height={220}>
            <ComposedChart data={usage.daily}>
              <CartesianGrid strokeDasharray="3 3" stroke="#E2E8F0" />
              <XAxis dataKey="day" tick={{ fontSize: 10 }} minTickGap={20} />
              <YAxis yAxisId="left"  tick={{ fontSize: 11 }} tickFormatter={(v) => `$${Number(v).toFixed(2)}`} />
              <YAxis yAxisId="right" orientation="right" tick={{ fontSize: 11 }} allowDecimals={false} />
              <Tooltip />
              <Area yAxisId="left"  type="monotone" dataKey="cost_usd" stroke="#0050A0" fill="#BFDBFE" fillOpacity={0.5} />
              <Line yAxisId="right" type="monotone" dataKey="calls"    stroke="#7C3AED" strokeWidth={2} dot={false} />
            </ComposedChart>
          </ResponsiveContainer>
        )}
      </section>

      {/* DEFERRED 14 + 15: cross-project debate health. Aggregated from the
          same agent_usage_events; per-project health pages already exist. */}
      {health && (
        <section className="rounded-lg border border-[#E2E8F0] bg-white p-4">
          <div className="mb-3 flex items-center justify-between text-sm font-semibold text-[#1A1A2E]">
            <span>{t("insights.debateHealthTitle")}</span>
            <span className="text-xs font-normal text-[#94A3B8]">
              {health.total_debate_turns} {t("insights.debateTurnsLabel")} · {t("insights.windowLast").replace("{days}", String(health.days))}
            </span>
          </div>
          <div className="grid gap-3 md:grid-cols-3">
            <Kpi label={t("insights.consensusRate")}     value={`${(health.consensus_rate * 100).toFixed(1)}%`} />
            <Kpi label={t("insights.fileCitationRate")}  value={`${(health.file_citation_rate * 100).toFixed(1)}%`} />
            <Kpi label={t("insights.projectsActive")}    value={String(health.by_project.filter((p) => p.debate_turns > 0).length)} />
          </div>

          {health.round_distribution.length > 0 && (
            <div className="mt-4">
              <div className="mb-2 text-xs text-[#64748B]">{t("insights.roundsDistribution")}</div>
              <ResponsiveContainer width="100%" height={140}>
                <BarChart data={health.round_distribution}>
                  <CartesianGrid strokeDasharray="3 3" stroke="#E2E8F0" />
                  <XAxis dataKey="rounds" tick={{ fontSize: 11 }} />
                  <YAxis tick={{ fontSize: 11 }} allowDecimals={false} />
                  <Tooltip />
                  <Bar dataKey="count" fill="#0EA5E9" radius={[4, 4, 0, 0]} />
                </BarChart>
              </ResponsiveContainer>
            </div>
          )}

          {health.by_project.some((p) => p.debate_turns > 0) && (
            <ul className="mt-4 divide-y divide-[#E2E8F0] rounded-md border border-[#E2E8F0]">
              {health.by_project
                .filter((p) => p.debate_turns > 0)
                .map((p) => {
                  const cRate = p.debate_turns > 0 ? p.consensus_turns / p.debate_turns : 0;
                  const fRate = p.debate_turns > 0 ? p.citation_turns  / p.debate_turns : 0;
                  return (
                    <li key={p.project_id} className="flex items-center justify-between gap-3 px-3 py-2 text-xs">
                      {p.project_deleted ? (
                        <span className="truncate font-medium text-[#94A3B8] line-through" title="Project has been deleted; historical debate metrics preserved.">
                          {p.project_name}
                        </span>
                      ) : (
                        <Link
                          href={`/projects/${p.project_id}?tab=insights`}
                          className="truncate font-medium text-[#1A1A2E] hover:text-[#0050A0] hover:underline"
                        >
                          {p.project_name}
                        </Link>
                      )}
                      <div className="flex flex-shrink-0 items-center gap-3 text-[#475569]">
                        <span>{p.debate_turns} {t("insights.turnsSuffix")}</span>
                        <span>{t("insights.consensusSuffix")} {(cRate * 100).toFixed(0)}%</span>
                        <span>{t("insights.citationSuffix")} {(fRate * 100).toFixed(0)}%</span>
                      </div>
                    </li>
                  );
                })}
            </ul>
          )}
        </section>
      )}

      <section className="rounded-lg border border-[#E2E8F0] bg-white">
        <div className="border-b border-[#E2E8F0] px-4 py-3 text-sm font-semibold text-[#1A1A2E]">
          {t("globalInsights.projectDetail")}
        </div>
        <ul className="divide-y divide-[#E2E8F0]">
          {usage.by_project.map((p, i) => (
            <li key={p.project_id} className="flex items-center justify-between gap-3 px-4 py-3">
              <div className="flex items-center gap-2 min-w-0">
                <span className="h-2 w-2 flex-shrink-0 rounded-full" style={{ background: PROJECT_COLORS[i % PROJECT_COLORS.length] }} />
                {/* Deleted projects (mig 0023 snapshot) — render as muted
                    plain text instead of a link, since clicking through to
                    a non-existent project would 404. */}
                {p.project_deleted ? (
                  <span className="truncate text-sm font-medium text-[#94A3B8] line-through" title="Project has been deleted; historical cost preserved.">
                    {p.project_name}
                  </span>
                ) : (
                  <Link href={`/projects/${p.project_id}?tab=insights`} className="truncate text-sm font-medium text-[#1A1A2E] hover:text-[#0050A0] hover:underline">
                    {p.project_name}
                  </Link>
                )}
              </div>
              <div className="flex flex-shrink-0 items-center gap-4 text-xs text-[#475569]">
                <span>${p.cost_usd.toFixed(2)}</span>
                <span>{p.calls} {t("globalInsights.callsShort")}</span>
                <span>{(p.tokens_in + p.tokens_out).toLocaleString()} {t("globalInsights.tokensShort")}</span>
                {!p.project_deleted && (
                  <Link href={`/projects/${p.project_id}?tab=insights`} className="text-[#0050A0]">
                    <ExternalLink size={12} />
                  </Link>
                )}
              </div>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}

function Kpi({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-[#E2E8F0] bg-white p-4">
      <div className="text-xs uppercase tracking-wider text-[#94A3B8]">{label}</div>
      <div className="mt-1 text-2xl font-semibold text-[#1A1A2E]">{value}</div>
    </div>
  );
}

function Empty() {
  return <div className="flex h-[200px] items-center justify-center text-sm text-[#94A3B8]">No data yet</div>;
}
