"use client";
import { AlertCircle, CheckCircle2, Info, X, AlertTriangle } from "lucide-react";
import { cn } from "@/lib/utils";
import { useToastStore, type ToastTone } from "@/lib/toast-store";

const toneStyles: Record<ToastTone, string> = {
  success: "border-[#BBF7D0] bg-[#F0FDF4] text-[#166534]",
  info: "border-[#BFDBFE] bg-[#EFF6FF] text-[#1D4ED8]",
  warning: "border-[#FDE68A] bg-[#FFFBEB] text-[#B45309]",
  error: "border-[#FECACA] bg-[#FEF2F2] text-[#991B1B]",
};

const toneIcons: Record<ToastTone, typeof CheckCircle2> = {
  success: CheckCircle2,
  info: Info,
  warning: AlertTriangle,
  error: AlertCircle,
};

export function Toaster() {
  const toasts = useToastStore((state) => state.toasts);
  const removeToast = useToastStore((state) => state.removeToast);

  if (toasts.length === 0) return null;

  return (
    <div className="pointer-events-none fixed right-4 top-4 z-[100] flex w-full max-w-sm flex-col gap-3">
      {toasts.map((toast) => {
        const Icon = toneIcons[toast.tone];
        return (
          <div
            key={toast.id}
            className={cn(
              "pointer-events-auto rounded-2xl border p-4 shadow-[0_18px_40px_rgba(15,23,42,0.14)] backdrop-blur-sm",
              toneStyles[toast.tone]
            )}
          >
            <div className="flex items-start gap-3">
              <div className="mt-0.5"><Icon size={18} /></div>
              <div className="min-w-0 flex-1">
                <div className="text-sm font-semibold">{toast.title}</div>
                {toast.description && (
                  <div className="mt-1 text-sm opacity-90">{toast.description}</div>
                )}
              </div>
              <button
                type="button"
                onClick={() => removeToast(toast.id)}
                className="rounded-lg p-1 opacity-70 transition hover:bg-black/5 hover:opacity-100"
              >
                <X size={15} />
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
