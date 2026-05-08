"use client";
import { useCallback, useEffect, useState } from "react";
import {
  ResponsiveContainer, AreaChart, Area, XAxis, YAxis, Tooltip, CartesianGrid, BarChart, Bar, Cell,
} from "recharts";
import { projects as projectsApi, type MetricsCost } from "@/lib/api";
import { useT } from "@/lib/i18n";

const AGENT_COLORS: Record<string, string> = {
  openclaw: "#0050A0",
  hermes: "#7C3AED",
};

const MODE_COLORS: Record<string, string> = {
  openclaw: "#0050A0",
  hermes: "#7C3AED",
  debate: "#F59E0B",
};

export function CostTab({ projectId }: { projectId: string }) {
  const [data, setData] = useState<MetricsCost | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>("");
  const t = useT();

  const refresh = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      setData(await projectsApi.metricsCost(projectId));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load cost metrics");
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  useEffect(() => {
    const timer = window.setTimeout(() => { void refresh(); }, 0);
    return () => window.clearTimeout(timer);
  }, [refresh]);

  if (loading && !data) return <div className="p-8 text-center text-[#94A3B8]">Loading…</div>;
  if (error) return <div className="p-8 text-center text-[#C8102E]">{error}</div>;
  if (!data) return null;

  // Reshape daily data into wide form for stacked area: { day, openclaw, hermes }
  const dailyMap = new Map<string, { day: string; openclaw: number; hermes: number }>();
  for (const row of data.daily) {
    const existing = dailyMap.get(row.day) ?? { day: row.day, openclaw: 0, hermes: 0 };
    if (row.agent === "openclaw") existing.openclaw += row.cost_usd;
    if (row.agent === "hermes") existing.hermes += row.cost_usd;
    dailyMap.set(row.day, existing);
  }
  const dailySorted = Array.from(dailyMap.values()).sort((a, b) => a.day.localeCompare(b.day));

  const totalIn = data.by_agent.reduce((s, r) => s + r.tokens_in, 0);
  const totalOut = data.by_agent.reduce((s, r) => s + r.tokens_out, 0);
  const totalCalls = data.by_agent.reduce((s, r) => s + r.calls, 0);

  return (
    <div className="p-6 space-y-6 overflow-auto">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-[#1A1A2E]">{t("cost.title")}</h2>
        <button onClick={() => void refresh()} className="text-xs text-[#0050A0] hover:underline">{t("common.refresh")}</button>
      </div>

      <div className="grid grid-cols-2 md:grid-cols-5 gap-3">
        <Kpi label={t("cost.totalCost")} value={`$${data.total_cost_usd.toFixed(2)}`} />
        <Kpi label={t("cost.inputTokens")} value={totalIn.toLocaleString()} />
        <Kpi label={t("cost.outputTokens")} value={totalOut.toLocaleString()} />
        <Kpi label={t("cost.agentCalls")} value={totalCalls.toLocaleString()} />
        <Kpi label={t("cost.avgPerCall")} value={totalCalls > 0 ? `$${(data.total_cost_usd / totalCalls).toFixed(4)}` : "—"} />
      </div>

      <div className="text-xs text-[#94A3B8] bg-[#FEF3C7] border border-[#FCD34D] rounded-md px-3 py-2">
        ⚠️ {t("cost.note")}
      </div>

      <Card title={t("cost.dailyTrend")} subtitle="USD per day · stacked by agent">
        {dailySorted.length === 0 ? (
          <Empty hint="No usage in the last 30 days" />
        ) : (
          <ResponsiveContainer width="100%" height={240}>
            <AreaChart data={dailySorted}>
              <CartesianGrid strokeDasharray="3 3" stroke="#E2E8F0" />
              <XAxis dataKey="day" tick={{ fontSize: 11 }} />
              <YAxis tick={{ fontSize: 11 }} tickFormatter={(v) => `$${v.toFixed(2)}`} />
              <Tooltip formatter={(v) => `$${Number(v).toFixed(4)}`} />
              <Area type="monotone" dataKey="openclaw" stackId="1" stroke="#0050A0" fill="#0050A0" fillOpacity={0.5} />
              <Area type="monotone" dataKey="hermes" stackId="1" stroke="#7C3AED" fill="#7C3AED" fillOpacity={0.5} />
            </AreaChart>
          </ResponsiveContainer>
        )}
      </Card>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <Card title={t("cost.byAgent")}>
          {data.by_agent.length === 0 ? (
            <Empty />
          ) : (
            <ResponsiveContainer width="100%" height={200}>
              <BarChart data={data.by_agent}>
                <CartesianGrid strokeDasharray="3 3" stroke="#E2E8F0" />
                <XAxis dataKey="agent" tick={{ fontSize: 12 }} />
                <YAxis tick={{ fontSize: 11 }} tickFormatter={(v) => `$${v.toFixed(2)}`} />
                <Tooltip formatter={(v) => `$${Number(v).toFixed(4)}`} />
                <Bar dataKey="cost_usd" radius={[4, 4, 0, 0]}>
                  {data.by_agent.map((r) => <Cell key={r.agent} fill={AGENT_COLORS[r.agent] ?? "#94A3B8"} />)}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          )}
        </Card>

        <Card title={t("cost.byMode")} subtitle="Debate is naturally more expensive (multiple rounds)">
          {data.by_mode.length === 0 ? (
            <Empty />
          ) : (
            <ResponsiveContainer width="100%" height={200}>
              <BarChart data={data.by_mode}>
                <CartesianGrid strokeDasharray="3 3" stroke="#E2E8F0" />
                <XAxis dataKey="mode" tick={{ fontSize: 12 }} />
                <YAxis tick={{ fontSize: 11 }} tickFormatter={(v) => `$${v.toFixed(2)}`} />
                <Tooltip formatter={(v) => `$${Number(v).toFixed(4)}`} />
                <Bar dataKey="cost_usd" radius={[4, 4, 0, 0]}>
                  {data.by_mode.map((r) => <Cell key={r.mode} fill={MODE_COLORS[r.mode] ?? "#94A3B8"} />)}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          )}
        </Card>
      </div>

      <Card title={t("cost.pricingInUse")}>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3 pt-2 text-xs">
          <Stat label="OpenClaw in" value={`$${data.pricing.openclaw_per_1k_in}/1k`} />
          <Stat label="OpenClaw out" value={`$${data.pricing.openclaw_per_1k_out}/1k`} />
          <Stat label="Hermes in" value={`$${data.pricing.hermes_per_1k_in}/1k`} />
          <Stat label="Hermes out" value={`$${data.pricing.hermes_per_1k_out}/1k`} />
        </div>
        <div className="text-[11px] text-[#94A3B8] mt-2">
          Tune via env: OPENCLAW_PRICE_PER_1K_INPUT/OUTPUT, HERMES_PRICE_PER_1K_INPUT/OUTPUT.
        </div>
      </Card>
    </div>
  );
}

function Kpi({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="rounded-lg border border-[#E2E8F0] bg-white p-4">
      <div className="text-xs text-[#94A3B8] uppercase tracking-wider">{label}</div>
      <div className="mt-1 text-2xl font-semibold text-[#1A1A2E]">{value}</div>
    </div>
  );
}
function Card({ title, subtitle, children }: { title: string; subtitle?: string; children: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-[#E2E8F0] bg-white p-4">
      <div className="mb-2">
        <div className="text-sm font-semibold text-[#1A1A2E]">{title}</div>
        {subtitle && <div className="text-[11px] text-[#94A3B8]">{subtitle}</div>}
      </div>
      {children}
    </div>
  );
}
function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md bg-[#F8FAFC] p-3">
      <div className="text-[11px] text-[#94A3B8] uppercase">{label}</div>
      <div className="text-base font-semibold text-[#1A1A2E] mt-0.5">{value}</div>
    </div>
  );
}
function Empty({ hint = "No data yet" }: { hint?: string }) {
  return <div className="h-[200px] flex items-center justify-center text-sm text-[#94A3B8]">{hint}</div>;
}
