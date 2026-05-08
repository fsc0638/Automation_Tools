"use client";
import { useCallback, useEffect, useState } from "react";
import {
  ResponsiveContainer, BarChart, Bar, XAxis, YAxis, Tooltip, CartesianGrid,
  PieChart, Pie, Cell,
  RadarChart, PolarGrid, PolarAngleAxis, PolarRadiusAxis, Radar,
} from "recharts";
import { projects as projectsApi, type MetricsSummary, type MetricsHealth } from "@/lib/api";
import { useT } from "@/lib/i18n";

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
  const t = useT();

  const refresh = useCallback(async () => {
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
  }, [projectId]);

  useEffect(() => {
    const timer = window.setTimeout(() => { void refresh(); }, 0);
    return () => window.clearTimeout(timer);
  }, [refresh]);

  if (loading && !metrics) {
    return <div className="p-8 text-center text-[#94A3B8]">Loading metrics…</div>;
  }
  if (error) {
    return <div className="p-8 text-center text-[#C8102E]">{error}</div>;
  }
  if (!metrics) return null;

  const { totals, mode_distribution, avg_chars_by_agent, consensus, debate_round_distribution, timing, file_citation, feedback_by_agent } = metrics;

  return (
    <div className="p-6 space-y-6 overflow-auto">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-[#1A1A2E]">{t("insights.title")}</h2>
        <button
          onClick={() => void refresh()}
          className="text-xs text-[#0050A0] hover:underline"
        >
          {t("common.refresh")}
        </button>
      </div>

      {health && (
        <ChartCard
          title={t("insights.healthScore")}
          subtitle={`${t("insights.healthDesc")} · ${health.indexed_files} ${t("insights.indexedFiles")} · Confidence ${health.confidence ?? "—"}/100`}
        >
          <div className="rounded-lg border border-[#DBEAFE] bg-[#F8FBFF] p-3 text-xs leading-5 text-[#475569]">
            <div className="font-semibold text-[#1A1A2E]">Evidence-based methodology</div>
            <div className="mt-1">{health.methodology ?? "Scores are calculated from indexed repository evidence."}</div>
            {health.limitations && health.limitations.length > 0 && (
              <ul className="mt-2 list-disc space-y-1 pl-5 text-[#64748B]">
                {health.limitations.map((item) => <li key={item}>{item}</li>)}
              </ul>
            )}
          </div>

          <div className="flex flex-col md:flex-row items-center gap-6 pt-4">
            <div className="flex flex-col items-center min-w-[160px]">
              <div className={`text-5xl font-bold ${
                health.score >= 80 ? "text-[#10B981]" : health.score >= 50 ? "text-[#F59E0B]" : "text-[#C8102E]"
              }`}>{health.score}</div>
              <div className="text-xs text-[#94A3B8] mt-1">Health / 100</div>
              <div className={`mt-2 rounded-full px-2.5 py-1 text-xs font-semibold ${
                (health.confidence ?? 0) >= 90 ? "bg-emerald-50 text-emerald-700" : "bg-amber-50 text-amber-700"
              }`}>Confidence {health.confidence ?? "—"}/100</div>
            </div>
            <div className="flex-1 w-full">
              <ResponsiveContainer width="100%" height={240}>
                <RadarChart data={health.dimensions.map(d => ({ label: d.label, score: d.score, confidence: d.confidence ?? 0 }))}>
                  <PolarGrid />
                  <PolarAngleAxis dataKey="label" tick={{ fontSize: 11 }} />
                  <PolarRadiusAxis domain={[0, 100]} tick={{ fontSize: 10 }} />
                  <Radar name="Score" dataKey="score" stroke="#0050A0" fill="#0050A0" fillOpacity={0.35} />
                  <Radar name="Confidence" dataKey="confidence" stroke="#10B981" fill="#10B981" fillOpacity={0.15} />
                  <Tooltip />
                </RadarChart>
              </ResponsiveContainer>
            </div>
          </div>

          {health.signals && (
            <div className="mt-3 grid grid-cols-2 gap-2 md:grid-cols-5">
              {Object.entries(health.signals).map(([key, value]) => (
                <div key={key} className="rounded-md bg-[#F8FAFC] p-2">
                  <div className="text-[10px] uppercase tracking-wide text-[#94A3B8]">{key.replaceAll("_", " ")}</div>
                  <div className="text-sm font-semibold text-[#1A1A2E]">{value}</div>
                </div>
              ))}
            </div>
          )}

          <div className="mt-4 grid grid-cols-1 gap-3 xl:grid-cols-2">
            {health.dimensions.map((d) => (
              <div key={d.key} className="rounded-lg border border-[#E2E8F0] bg-white p-3 text-xs shadow-sm">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <div className="font-semibold text-[#1A1A2E]">{d.label}</div>
                    <div className="mt-0.5 text-[11px] text-[#94A3B8]">{d.measured_by}</div>
                  </div>
                  <div className="flex items-center gap-2">
                    <span className={`rounded-full px-2 py-0.5 text-[10px] font-semibold ${
                      d.level === "Low" ? "bg-green-100 text-green-700"
                      : d.level === "Medium" ? "bg-yellow-100 text-yellow-700"
                      : "bg-red-100 text-red-700"
                    }`}>{d.level} risk</span>
                    <span className={`rounded-full px-2 py-0.5 text-[10px] font-semibold ${
                      (d.confidence ?? 0) >= 90 ? "bg-emerald-50 text-emerald-700" : "bg-amber-50 text-amber-700"
                    }`}>Conf {d.confidence ?? "—"}</span>
                  </div>
                </div>
                <div className="mt-3 flex items-end gap-3">
                  <div className="text-2xl font-bold text-[#0050A0]">{d.score}</div>
                  <div className="mb-1 h-2 flex-1 overflow-hidden rounded-full bg-[#F1F5F9]">
                    <div className="h-full rounded-full bg-[#0050A0]" style={{ width: `${Math.max(0, Math.min(100, d.score))}%` }} />
                  </div>
                </div>
                <div className="mt-2 rounded-md bg-[#F8FAFC] p-2 text-[11px] text-[#475569]">
                  <span className="font-semibold text-[#1A1A2E]">Formula: </span>{d.formula ?? "—"}
                </div>
                <div className="mt-2 text-[11px] text-[#64748B]">{d.evidence}</div>
                {d.evidence_items && d.evidence_items.length > 0 && (
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {d.evidence_items.map((item) => (
                      <span key={item} className="rounded-full border border-[#E2E8F0] bg-[#FBFCFE] px-2 py-0.5 text-[10px] text-[#64748B]">{item}</span>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        </ChartCard>
      )}

      {/* KPI cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <KpiCard label={t("insights.totalConversations")} value={totals.conversations} />
        <KpiCard label={t("insights.totalMessages")} value={totals.messages} />
        <KpiCard label={t("insights.agentReplies")} value={totals.agent_messages} />
        <KpiCard
          label={t("insights.fileCited")}
          value={`${(file_citation.rate * 100).toFixed(0)}%`}
          sub={`${file_citation.with_citation}/${file_citation.total}`}
        />
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <ChartCard title={t("insights.modeDistribution")}>
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
          title={t("insights.avgReplyLength")}
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
          title={t("insights.consensusRate")}
          subtitle={t("insights.consensusDesc")}
        >
          {consensus.debate_finals === 0 ? (
            <Empty hint="No Debate sessions yet" />
          ) : (
            <Gauge value={consensus.rate} sub={`${consensus.with_consensus}/${consensus.debate_finals}`} />
          )}
        </ChartCard>

        <ChartCard title={t("insights.roundDistribution")}>
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

      {feedback_by_agent.length > 0 && (
        <ChartCard title={t("insights.satisfaction")} subtitle={t("insights.satisfactionDesc")}>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3 pt-2">
            {feedback_by_agent.map((f) => (
              <div key={f.agent} className="rounded-md bg-[#F8FAFC] p-3 flex items-center justify-between">
                <div>
                  <div className="text-sm font-medium text-[#1A1A2E] capitalize">{f.agent}</div>
                  <div className="text-[11px] text-[#94A3B8]">{f.thumbs_up} 👍 · {f.thumbs_down} 👎 · {f.total} total</div>
                </div>
                <div className="text-right">
                  <div className={`text-2xl font-bold ${
                    f.satisfaction_rate >= 0.8 ? "text-[#10B981]"
                    : f.satisfaction_rate >= 0.5 ? "text-[#F59E0B]"
                    : "text-[#C8102E]"
                  }`}>
                    {(f.satisfaction_rate * 100).toFixed(0)}%
                  </div>
                  <div className="text-[10px] text-[#94A3B8]">satisfaction</div>
                </div>
              </div>
            ))}
          </div>
        </ChartCard>
      )}

      <ChartCard title={t("insights.latency")} subtitle={t("insights.latencyDesc")}>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3 pt-2">
          <Stat label={t("insights.avgTtft")} value={fmtMs(timing.avg_ttft_ms)} />
          <Stat label={t("insights.avgTotal")} value={fmtMs(timing.avg_total_ms)} />
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
