"use client";
import { Check } from "lucide-react";
import { type MeetingTimeSlot } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

function formatHM(iso: string): string {
  const d = new Date(iso);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function TimeSlotPanel({
  slots,
  selectedStartIso,
  onPick,
}: {
  slots: MeetingTimeSlot[];
  selectedStartIso: string | null;
  onPick: (slot: MeetingTimeSlot) => void;
}) {
  const t = useT();

  if (slots.length === 0) {
    return (
      <div className="rounded-xl border border-dashed border-[#E2E8F0] p-4 text-center text-[12px] text-[#94A3B8]">
        無可用時段建議
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {slots.map((s) => {
        const isApplied = selectedStartIso === s.start_at;
        const allFree = s.available_count === s.total_attendees;
        const someBusy = !allFree && s.available_count > 0;
        return (
          <button
            key={s.start_at}
            type="button"
            onClick={() => onPick(s)}
            className={cn(
              "flex w-full items-center justify-between rounded-xl border px-3 py-3 text-left transition",
              isApplied
                ? "border-[#10B981] bg-[#ECFDF5]"
                : "border-[#E2E8F0] bg-white hover:border-[#CBD5E1] hover:bg-[#F8FAFC]"
            )}
          >
            <div className="flex items-start gap-3">
              <span
                className={cn(
                  "mt-0.5 flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full border",
                  isApplied
                    ? "border-[#10B981] bg-[#10B981] text-white"
                    : "border-[#CBD5E1] bg-white"
                )}
              >
                {isApplied && <Check size={11} strokeWidth={3} />}
              </span>
              <div>
                <div className="text-[13px] font-semibold text-[#1A1A2E]">
                  {formatHM(s.start_at)} - {formatHM(s.end_at)}
                </div>
                <div className="mt-0.5 text-[11px] text-[#64748B]">
                  {allFree
                    ? t("meetings.recommended.allFreeSync").replace("{count}", String(s.available_count))
                    : someBusy
                      ? t("meetings.recommended.busyHint").replace(
                          "{who}",
                          s.busy_names.slice(0, 2).join(" 與 ")
                        )
                      : "全員忙碌"}
                </div>
              </div>
            </div>
            <span
              className={cn(
                "rounded-md px-2 py-0.5 text-[10px] font-medium",
                isApplied
                  ? "bg-[#10B981] text-white"
                  : "border border-[#CBD5E1] text-[#64748B]"
              )}
            >
              {isApplied ? t("meetings.recommended.applied") : t("meetings.recommended.pending")}
            </span>
          </button>
        );
      })}
    </div>
  );
}
