"use client";
import { useState } from "react";
import { meetings as meetingsApi, type MeetingAttendee } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { formatDateTime } from "./meeting-utils";

export function AttendeeSignoff({
  meetingId,
  attendees,
  onChange,
}: {
  meetingId: string;
  attendees: MeetingAttendee[];
  onChange: () => void;
}) {
  const t = useT();
  const [busyEmail, setBusyEmail] = useState<string | null>(null);

  async function confirm(email: string) {
    setBusyEmail(email);
    try {
      await meetingsApi.confirmAttendance(meetingId, email);
      onChange();
    } finally {
      setBusyEmail(null);
    }
  }

  async function dispute(email: string) {
    const note = window.prompt("請填寫異議原因");
    if (note === null) return;
    setBusyEmail(email);
    try {
      await meetingsApi.disputeAttendance(meetingId, email, note);
      onChange();
    } finally {
      setBusyEmail(null);
    }
  }

  const done = attendees.filter((a) => a.confirmation_status !== "pending").length;

  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
      <header className="flex items-center justify-between">
        <div>
          <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
            {t("meetings.attendees.title")}
          </div>
          <div className="mt-1 text-[12px] text-[#94A3B8]">{t("meetings.attendees.desc")}</div>
        </div>
        <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2 py-0.5 text-[11px] text-[#475569]">
          {t("meetings.attendees.replied")
            .replace("{done}", String(done))
            .replace("{total}", String(attendees.length))}
        </span>
      </header>

      <ul className="mt-4 divide-y divide-[#E2E8F0]">
        {attendees.map((a) => {
          const isPending = a.confirmation_status === "pending";
          const isDisputed = a.confirmation_status === "disputed";
          return (
            <li key={a.email} className="flex items-center justify-between gap-3 py-3">
              <div className="min-w-0">
                <div className="text-[13px] font-semibold text-[#1A1A2E]">
                  {a.display_name || a.email}
                  {a.role_label && <span className="ml-1 font-normal text-[#94A3B8]">· {a.role_label}</span>}
                </div>
                <div className="mt-0.5 text-[11px] text-[#94A3B8]">
                  {t("meetings.attendees.lastAction")}：
                  {a.last_action_at ? formatDateTime(a.last_action_at) : "—"}
                </div>
                {a.dispute_note && (
                  <div className="mt-1 text-[11px] text-[#991B1B]">「{a.dispute_note}」</div>
                )}
              </div>

              {isPending ? (
                <div className="flex flex-shrink-0 gap-1">
                  <button
                    type="button"
                    disabled={busyEmail === a.email}
                    onClick={() => void confirm(a.email)}
                    className="rounded-md bg-[#10B981] px-2.5 py-1 text-[11px] font-medium text-white hover:bg-[#059669] disabled:opacity-60"
                  >
                    {t("meetings.attendees.confirmBtn")}
                  </button>
                  <button
                    type="button"
                    disabled={busyEmail === a.email}
                    onClick={() => void dispute(a.email)}
                    className="rounded-md border border-[#FCA5A5] bg-white px-2.5 py-1 text-[11px] font-medium text-[#C8102E] hover:bg-[#FEE2E2] disabled:opacity-60"
                  >
                    {t("meetings.attendees.disputeBtn")}
                  </button>
                </div>
              ) : (
                <span
                  className={cn(
                    "flex-shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium",
                    isDisputed
                      ? "bg-[#FEE2E2] text-[#991B1B]"
                      : "bg-[#D1FAE5] text-[#065F46]"
                  )}
                >
                  {isDisputed ? t("meetings.attendees.disputed") : t("meetings.attendees.confirmed")}
                </span>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
