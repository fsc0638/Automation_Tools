"use client";
import Link from "next/link";
import { useState } from "react";
import { ArrowRight, Plus } from "lucide-react";
import { type MeetingTaskImpact, type MeetingImpactType } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { AddTaskImpactModal } from "./AddTaskImpactModal";

const TYPE_STYLES: Record<MeetingImpactType, string> = {
  new:      "bg-[#D1FAE5] text-[#065F46]",
  update:   "bg-[#DBEAFE] text-[#1E40AF]",
  progress: "bg-[#EDE9FE] text-[#5B21B6]",
};

export function TaskImpactList({
  meetingId,
  impacts,
  defaultProjectId,
  onChange,
}: {
  meetingId: string;
  impacts: MeetingTaskImpact[];
  defaultProjectId: string | null;
  onChange: () => void;
}) {
  const t = useT();
  const [showHidden, setShowHidden] = useState(false);
  const [adding, setAdding] = useState(false);

  const visible = impacts.filter((i) => showHidden || !i.is_hidden);
  const hiddenCount = impacts.filter((i) => i.is_hidden).length;

  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <header className="flex items-center justify-between">
        <div>
          <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
            {t("meetings.impacts.title")}
          </div>
          <div className="mt-1 text-[12px] text-[#94A3B8]">{t("meetings.impacts.desc")}</div>
        </div>
        <div className="flex items-center gap-2">
          <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2 py-0.5 text-[11px] text-[#475569]">
            {t("meetings.impacts.changes").replace("{n}", String(impacts.length))}
          </span>
          <button
            type="button"
            onClick={() => setAdding(true)}
            className="inline-flex items-center gap-1 rounded-md border border-[#E2E8F0] bg-white px-2 py-1 text-[11px] font-medium text-[#475569] hover:bg-[#F8FAFC]"
          >
            <Plus size={11} /> 新增
          </button>
        </div>
      </header>

      <AddTaskImpactModal
        meetingId={meetingId}
        open={adding}
        defaultProjectId={defaultProjectId}
        onClose={() => setAdding(false)}
        onAdded={onChange}
      />

      <ul className="mt-4 space-y-2">
        {visible.length === 0 ? (
          <li className="rounded-lg border border-dashed border-[#E2E8F0] px-3 py-4 text-center text-[12px] text-[#94A3B8]">
            尚未連結任何任務
          </li>
        ) : (
          visible.map((i) => (
            <li key={i.id} className="rounded-xl border border-[#E2E8F0] bg-white p-3">
              <div className="flex items-start justify-between gap-3">
                <div className="flex items-start gap-2 min-w-0">
                  <span
                    className={cn(
                      "flex-shrink-0 rounded-md px-1.5 py-0.5 text-[11px] font-semibold",
                      TYPE_STYLES[i.impact_type]
                    )}
                  >
                    {t(`meetings.impacts.type.${i.impact_type}`)}
                  </span>
                  <div className="min-w-0">
                    <div className="text-[13px] font-medium text-[#1A1A2E]">{i.description}</div>
                    {i.impact_type === "progress" && i.progress_from != null && i.progress_to != null && (
                      <div className="mt-1 inline-flex items-center gap-1 rounded-md bg-[#F1F5F9] px-2 py-0.5 text-[11px] text-[#475569]">
                        進度：{i.progress_from}% → {i.progress_to}%
                      </div>
                    )}
                  </div>
                </div>
                {i.project_id && i.task_id ? (
                  <Link
                    href={`/projects/${i.project_id}?tab=roadmap&task=${i.task_id}`}
                    className="flex-shrink-0 rounded-md p-1 text-[#94A3B8] hover:bg-[#F1F5F9] hover:text-[#1A1A2E]"
                  >
                    <ArrowRight size={14} />
                  </Link>
                ) : null}
              </div>
            </li>
          ))
        )}
      </ul>

      {hiddenCount > 0 && (
        <button
          type="button"
          onClick={() => setShowHidden((v) => !v)}
          className="mt-3 text-[12px] text-[#0050A0] hover:underline"
        >
          {showHidden
            ? "收起隱藏項"
            : t("meetings.impacts.hiddenHint").replace("{n}", String(hiddenCount))}
        </button>
      )}
    </div>
  );
}
