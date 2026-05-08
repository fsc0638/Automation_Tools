"use client";
import { useEffect, useState } from "react";
import {
  ResponsiveContainer, BarChart, Bar, XAxis, YAxis, Tooltip, CartesianGrid,
  PieChart, Pie, Cell,
  RadarChart, PolarGrid, PolarAngleAxis, PolarRadiusAxis, Radar,
} from "recharts";
import { projects as projectsApi, type MetricsSummary, type MetricsHealth } from "@/lib/api";

const MODE_COLORS: Record<string, string> = {
  openclaw: "#0050A0",
  hermes: "#7C3AED",
  debate: "#F59E0B",
};

const AGENT_COLORS: Record<string, string> = {
  openclaw: "#0050A0",
  hermes: "#7C3AED",
};

export function InsightsTab({ projectId }: { projectId: string }) {
  const [metrics, setMetrics] = useState<MetricsSummary | null>(null);
  const [health, setHealth] = useState<MetricsHealth | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>("");

  async function refresh() {
    setLoading(true);
    setError("");
    try {
      const [m, h] = await Promise.all([
        projectsApi.metricsSummary(projectId),
        projectsApi.metricsHealth(projectId).catch(() => null),
      ]);
      setMetrics(m);
      setHealth(h);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load metrics");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { void refresh(); }, [projectId]);

  if (loading && !metrics) {
    return <div className="p-8 text-center text-[#94A3B8]">Loading metrics…</div>;
  }
  if (error) {
    return <div className="p-8 text-center text-[#C8102E]">{error}</div>;
  }
  if (!metrics) return null;

  const { totals, mode_distribution, avg_chars_by_agent, consensus, debate_round_distribution, timing, file_citation } = metrics;

  return (
    <div className="p-6 space-y-6 overflow-auto">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-[#1A1A2E]">Insights</h2>
        <button
          onClick={() => void refresh()}
          className="text-xs text-[#0050A0] hover:underline"
        >
          Refresh
        </button>
      </div>

      {health && (
        <ChartCard title="Project Health Score" subtitle={`Composite score across 5 risk dimensions · ${health.indexed_files} indexed files`}>
          <div className="flex flex-col md:flex-row items-center gap-6 pt-2">
            <div className="flex flex-col items-center min-w-[140px]">
              <div className={`text-5xl font-bold ${
                health.score >= 80 ? "text-[#10B981]" : health.score >= 50 ? "text-[#F59E0B]" : "text-[#C8102E]"
              }`}>{health.score}</div>
              <div className="text-xs text-[#94A3B8] mt-1">/ 100</div>
            </div>
            <div className="flex-1 w-full">
              <ResponsiveContainer width="100%" height={220}>
                <RadarChart data={health.dimensions.map(d => ({ label: d.label, score: d.score }))}>
                  <PolarGrid />
                  <PolarAngleAxis dataKey="label" tick={{ fontSize: 11 }} />
                  <PolarRadiusAxis domain={[0, 100]} tick={{ fontSize: 10 }} />
                  <Radar name="Score" dataKey="score" stroke="#0050A0" fill="#0050A0" fillOpacity={0.4} />
                  <Tooltip />
                </RadarChart>
              </ResponsiveContainer>
            </div>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-5 gap-2 mt-3 text-xs">
            {health.dimensions.map((d) => (
              <div key={d.key} className="rounded-md bg-[#F8FAFC] p-2">
                <div className="flex items-center justify-between mb-0.5">
                  <span className="font-medium text-[#1A1A2E]">{d.label}</span>
                  <span className={`text-[10px] px-1.5 py-0.5 rounded-full ${
                    d.level === "Low" ? "bg-green-100 text-green-700"
                    : d.level === "Medium" ? "bg-yellow-100 text-yellow-700"
                    : "bg-red-100 text-red-700"
                  }`}>{d.level}</span>
                </div>
                <div className="text-base font-semibold text-[#0050A0]">{d.score}</div>
                <div className="text-[10px] text-[#94A3B8] mt-0.5 line-clamp-2">{d.evidence}</div>
              </div>
            ))}
          </div>
        </ChartCard>
      )}

      {/* KPI cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <KpiCard label="Conversations" value={totals.conversations} />
        <KpiCard label="Total messages" value={totals.messages} />
        <KpiCard label="Agent replies" value={totals.agent_messages} />
        <KpiCard
          label="File-cited replies"
          value={`${(file_citation.rate * 100).toFixed(0)}%`}
          sub={`${file_citation.with_citation}/${file_citation.total}`}
        />
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <ChartCard title="Conversation mode distribution">
          {mode_distribution.length === 0 ? (
            <Empty />
          ) : (
            <ResponsiveContainer width="100%" height={200}>
              <PieChart>
                <Pie
                  data={mode_distribution}
                  dataKey="count"
                  nameKey="mode"
                  outerRadius={70}
                  label={(props) => {
                    const m = (props as unknown as { mode?: string; count?: number; name?: string; value?: number });
                    const label = m.mode ?? m.name ?? "";
                    const count = m.count ?? m.value ?? 0;
                    return `${label} (${count})`;
                  }}
                >
                  {mode_distribution.map((entry) => (
                    <Cell key={entry.mode} fill={MODE_COLORS[entry.mode] ?? "#94A3B8"} />
                  ))}
                </Pie>
                <Tooltip />
              </PieChart>
            </ResponsiveContainer>
          )}
        </ChartCard>

        <ChartCard
          title="Average reply length per agent"
          subtitle="characters · last 90 days"
        >
          {avg_chars_by_agent.length === 0 ? (
            <Empty />
          ) : (
            <ResponsiveContainer width="100%" height={200}>
              <BarChart data={avg_chars_by_agent}>
                <CartesianGrid strokeDasharray="3 3" stroke="#E2E8F0" />
                <XAxis dataKey="agent" tick={{ fontSize: 12 }} />
                <YAxis tick={{ fontSize: 11 }} />
                <Tooltip
                  formatter={(value) => [`${Number(value).toFixed(0)} chars`, "Average"]}
                />
                <Bar dataKey="avg_chars" radius={[4, 4, 0, 0]}>
                  {avg_chars_by_agent.map((entry) => (
                    <Cell
                      key={entry.agent}
                      fill={AGENT_COLORS[entry.agent] ?? "#94A3B8"}
                    />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          )}
        </ChartCard>

        <ChartCard
          title="Debate consensus rate"
          subtitle="ratio of Final turns ending with consensus"
        >
          {consensus.debate_finals === 0 ? (
            <Empty hint="No Debate sessions yet" />
          ) : (
            <Gauge value={consensus.rate} sub={`${consensus.with_consensus}/${consensus.debate_finals}`} />
          )}
        </ChartCard>

        <ChartCard title="Debate round distribution">
          {debate_round_distribution.length === 0 ? (
            <Empty hint="No Debate rounds recorded" />
          ) : (
            <ResponsiveContainer width="100%" height={200}>
              <BarChart data={debate_round_distribution}>
                <CartesianGrid strokeDasharray="3 3" stroke="#E2E8F0" />
                <XAxis dataKey="round" tick={{ fontSize: 12 }} label={{ value: "Round", position: "insideBottom", offset: -2, fontSize: 11 }} />
                <YAxis tick={{ fontSize: 11 }} />
                <Tooltip />
                <Bar dataKey="count" fill="#0050A0" radius={[4, 4, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          )}
        </ChartCard>
      </div>

      <ChartCard title="Latency" subtitle="across all agent calls">
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3 pt-2">
          <Stat label="Avg TTFT" value={fmtMs(timing.avg_ttft_ms)} />
          <Stat label="Avg total" value={fmtMs(timing.avg_total_ms)} />
          <Stat label="p50" value={fmtMs(timing.p50_total_ms)} />
          <Stat label="p95" value={fmtMs(timing.p95_total_ms)} />
        </div>
      </ChartCard>
    </div>
  );
}

function fmtMs(v: number | null): string {
  if (v == null) return "—";
  if (v >= 1000) return `${(v / 1000).toFixed(1)}s`;
  return `${Math.round(v)}ms`;
}

function KpiCard({ label, value, sub }: { label: string; value: string | number; sub?: string }) {
  return (
    <div className="rounded-lg border border-[#E2E8F0] bg-white p-4">
      <div className="text-xs text-[#94A3B8] uppercase tracking-wider">{label}</div>
      <div className="mt-1 text-2xl font-semibold text-[#1A1A2E]">{value}</div>
      {sub && <div className="text-[11px] text-[#94A3B8] mt-0.5">{sub}</div>}
    </div>
  );
}

function ChartCard({ title, subtitle, children }: { title: string; subtitle?: string; children: React.ReactNode }) {
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

function Gauge({ value, sub }: { value: number; sub?: string }) {
  const pct = Math.round(value * 100);
  return (
    <div className="flex flex-col items-center justify-center py-4">
      <div className="text-4xl font-bold text-[#0050A0]">{pct}%</div>
      {sub && <div className="text-xs text-[#94A3B8] mt-1">{sub}</div>}
      <div className="w-full mt-3 h-2 bg-[#F1F5F9] rounded-full overflow-hidden">
        <div className="h-full bg-[#0050A0] transition-all" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function Empty({ hint = "No data yet" }: { hint?: string }) {
  return <div className="h-[200px] flex items-center justify-center text-sm text-[#94A3B8]">{hint}</div>;
}
