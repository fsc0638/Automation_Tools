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

// Palette for custom agents not in AGENT_COLORS. Cycles when there are more agents than slots.
const CUSTOM_PALETTE = [
  "#059669", "#D97706", "#DC2626", "#2563EB", "#DB2777",
  "#0891B2", "#16A34A", "#EA580C", "#7C3AED", "#0284C7",
];

function agentColor(agent: string, dynamicIndex: number): string {
  return AGENT_COLORS[agent] ?? CUSTOM_PALETTE[dynamicIndex % CUSTOM_PALETTE.length];
}

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

  if (loading && !data) return <div className="p-8 text-center text-[#94A3B8]">{t("common.loading")}</div>;
  if (error) return <div className="p-8 text-center text-[#C8102E]">{error}</div>;
  if (!data) return null;

  // Collect agents in a stable order: known first-party agents first, then custom sorted alphabetically.
  const knownOrder = ["openclaw", "hermes"];
  const allAgents = [
    ...knownOrder.filter(a => data.daily.some(r => r.agent === a)),
    ...Array.from(new Set(data.daily.map(r => r.agent)))
      .filter(a => !knownOrder.includes(a))
      .sort(),
  ];

  // Assign a dynamic palette index to each agent that's not in AGENT_COLORS.
  let customIdx = 0;
  const agentColorMap = new Map<string, string>();
  for (const a of allAgents) {
    agentColorMap.set(a, agentColor(a, AGENT_COLORS[a] ? 0 : customIdx));
    if (!AGENT_COLORS[a]) customIdx++;
  }

  // Reshape daily data into wide form for stacked area: { day, [agent]: cost }
  type DailyWideRow = { day: string; [agent: string]: number | string };
  const dailyMap = new Map<string, DailyWideRow>();
  for (const row of data.daily) {
    if (!dailyMap.has(row.day)) dailyMap.set(row.day, { day: row.day });
    const entry = dailyMap.get(row.day)!;
    entry[row.agent] = ((entry[row.agent] as number | undefined) ?? 0) + row.cost_usd;
  }
  const dailySorted = Array.from(dailyMap.values()).sort((a, b) => a.day.localeCompare(b.day));

  const totalIn = data.by_agent.reduce((s, r) => s + r.tokens_in, 0);
  const totalOut = data.by_agent.reduce((s, r) => s + r.tokens_out, 0);
  const totalCalls = data.by_agent.reduce((s, r) => s + r.calls, 0);

  return (
    <div className="p-6 space-y-6 overflow-auto">
      <div className="flex items-center justify-between">
        <h2 className="type-section-title text-[1.4rem]">{t("cost.title")}</h2>
        <button onClick={() => void refresh()} className="text-[13px] text-[#0050A0] hover:underline">{t("common.refresh")}</button>
      </div>

      <div className="grid grid-cols-2 md:grid-cols-5 gap-3">
        <Kpi label={t("cost.totalCost")} value={`$${data.total_cost_usd.toFixed(2)}`} />
        <Kpi label={t("cost.inputTokens")} value={totalIn.toLocaleString()} />
        <Kpi label={t("cost.outputTokens")} value={totalOut.toLocaleString()} />
        <Kpi label={t("cost.agentCalls")} value={totalCalls.toLocaleString()} />
        <Kpi label={t("cost.avgPerCall")} value={totalCalls > 0 ? `$${(data.total_cost_usd / totalCalls).toFixed(4)}` : "—"} />
      </div>

      <div className="rounded-xl border border-[#FCD34D] bg-[#FEF3C7] px-4 py-3 text-[13px] leading-6 text-[#92400E]">
        ⚠️ {t("cost.note")}
      </div>

      <Card title={t("cost.dailyTrend")} subtitle="USD per day · stacked by agent">
        {dailySorted.length === 0 ? (
          <Empty hint={t("cost.noUsage")} />
        ) : (
          <ResponsiveContainer width="100%" height={240}>
            <AreaChart data={dailySorted}>
              <CartesianGrid strokeDasharray="3 3" stroke="#E2E8F0" />
              <XAxis dataKey="day" tick={{ fontSize: 11 }} />
              <YAxis tick={{ fontSize: 11 }} tickFormatter={(v) => `$${v.toFixed(2)}`} />
              <Tooltip formatter={(v) => `$${Number(v).toFixed(4)}`} />
              {allAgents.map(agent => {
                const color = agentColorMap.get(agent) ?? "#94A3B8";
                return (
                  <Area
                    key={agent}
                    type="monotone"
                    dataKey={agent}
                    stackId="1"
                    stroke={color}
                    fill={color}
                    fillOpacity={0.5}
                    name={agent}
                  />
                );
              })}
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
                  {data.by_agent.map((r) => <Cell key={r.agent} fill={agentColorMap.get(r.agent) ?? "#94A3B8"} />)}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          )}
        </Card>

        <Card title={t("cost.byMode")} subtitle={t("cost.debateExpensiveNote")}>
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
        <div className="grid grid-cols-2 gap-3 pt-2 text-[13px] md:grid-cols-4">
          <Stat label={t("cost.openclawIn")} value={`$${data.pricing.openclaw_per_1k_in}/1k`} />
          <Stat label={t("cost.openclawOut")} value={`$${data.pricing.openclaw_per_1k_out}/1k`} />
          <Stat label={t("cost.hermesIn")} value={`$${data.pricing.hermes_per_1k_in}/1k`} />
          <Stat label={t("cost.hermesOut")} value={`$${data.pricing.hermes_per_1k_out}/1k`} />
        </div>
        <div className="mt-2 text-[13px] leading-6 text-[#94A3B8]">
          {t("cost.envTuningNote")}
        </div>
      </Card>
    </div>
  );
}

function Kpi({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-4 shadow-sm">
      <div className="text-[12px] text-[#94A3B8] tracking-[0.05em]">{label}</div>
      <div className="mt-2 text-[1.75rem] font-semibold tracking-[-0.02em] text-[#1A1A2E]">{value}</div>
    </div>
  );
}
function Card({ title, subtitle, children }: { title: string; subtitle?: string; children: React.ReactNode }) {
  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-4 shadow-sm">
      <div className="mb-2">
        <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">{title}</div>
        {subtitle && <div className="mt-1 text-[13px] leading-6 text-[#94A3B8]">{subtitle}</div>}
      </div>
      {children}
    </div>
  );
}
function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl bg-[#F8FAFC] p-3">
      <div className="text-[12px] text-[#94A3B8] tracking-[0.05em]">{label}</div>
      <div className="mt-1 text-[15px] font-semibold text-[#1A1A2E]">{value}</div>
    </div>
  );
}
function Empty({ hint = "No data yet" }: { hint?: string }) {
  return <div className="h-[200px] flex items-center justify-center text-sm text-[#94A3B8]">{hint}</div>;
}
