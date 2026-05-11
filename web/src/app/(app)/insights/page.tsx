"use client";

import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { BarChart3, ExternalLink } from "lucide-react";
import {
  ResponsiveContainer, BarChart, Bar, XAxis, YAxis, Tooltip, CartesianGrid,
  Cell, ComposedChart, Area, Line,
} from "recharts";
import { userViews, type UserUsage } from "@/lib/api";
import { useT } from "@/lib/i18n";

const PROJECT_COLORS = ["#0050A0", "#7C3AED", "#F59E0B", "#10B981", "#C8102E", "#0EA5E9", "#EC4899"];

/**
 * B4: Global Insights — cross-project aggregation of agent_usage_events.
 * Shows total cost / tokens / calls, a per-project breakdown bar, and
 * a 30-day daily trend. Per-project drill-down is one click away via
 * project cards.
 */
export default function GlobalInsightsPage() {
  const t = useT();
  const [usage, setUsage] = useState<UserUsage | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      setUsage(await userViews.usage());
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load");
    } finally {
      setLoading(false);
    }
  }, []);

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
          <p className="text-xs text-[#94A3B8]">{t("globalInsights.subtitle")}</p>
        </div>
        <button onClick={() => void refresh()} className="text-xs text-[#0050A0] hover:underline">{t("common.refresh")}</button>
      </header>

      <section className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Kpi label={t("globalInsights.totalCost")}   value={`$${usage.total_cost_usd.toFixed(2)}`} />
        <Kpi label={t("globalInsights.totalCalls")}  value={usage.total_calls.toLocaleString()} />
        <Kpi label={t("globalInsights.tokensIn")}    value={usage.total_tokens_in.toLocaleString()} />
        <Kpi label={t("globalInsights.tokensOut")}   value={usage.total_tokens_out.toLocaleString()} />
      </section>

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

      <section className="rounded-lg border border-[#E2E8F0] bg-white">
        <div className="border-b border-[#E2E8F0] px-4 py-3 text-sm font-semibold text-[#1A1A2E]">
          {t("globalInsights.projectDetail")}
        </div>
        <ul className="divide-y divide-[#E2E8F0]">
          {usage.by_project.map((p, i) => (
            <li key={p.project_id} className="flex items-center justify-between gap-3 px-4 py-3">
              <div className="flex items-center gap-2 min-w-0">
                <span className="h-2 w-2 flex-shrink-0 rounded-full" style={{ background: PROJECT_COLORS[i % PROJECT_COLORS.length] }} />
                <Link href={`/projects/${p.project_id}`} className="truncate text-sm font-medium text-[#1A1A2E] hover:text-[#0050A0] hover:underline">
                  {p.project_name}
                </Link>
              </div>
              <div className="flex flex-shrink-0 items-center gap-4 text-xs text-[#475569]">
                <span>${p.cost_usd.toFixed(2)}</span>
                <span>{p.calls} {t("globalInsights.callsShort")}</span>
                <span>{(p.tokens_in + p.tokens_out).toLocaleString()} {t("globalInsights.tokensShort")}</span>
                <Link href={`/projects/${p.project_id}`} className="text-[#0050A0]">
                  <ExternalLink size={12} />
                </Link>
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
