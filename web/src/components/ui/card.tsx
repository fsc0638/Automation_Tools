import { type HTMLAttributes, type ReactNode } from "react";
import { cn } from "@/lib/utils";

interface CardProps extends HTMLAttributes<HTMLDivElement> {
  tone?: "default" | "muted" | "raised";
}

export function Card({ className, children, tone = "default", ...props }: CardProps) {
  return (
    <div
      className={cn(
        "rounded-2xl border shadow-sm",
        tone === "default" && "border-[#E2E8F0] bg-white",
        tone === "muted" && "border-[#E2E8F0] bg-[#FBFCFE]",
        tone === "raised" && "border-[#D6DFEA] bg-white shadow-[0_12px_40px_rgba(15,23,42,0.06)]",
        className
      )}
      {...props}
    >
      {children}
    </div>
  );
}

export function CardHeader({ className, children, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={cn("border-b border-[#E2E8F0] px-6 py-4", className)} {...props}>
      {children}
    </div>
  );
}

export function CardContent({ className, children, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={cn("px-6 py-5", className)} {...props}>
      {children}
    </div>
  );
}

export function SectionEmpty({
  title,
  description,
  action,
  className,
}: {
  title: string;
  description: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("rounded-2xl border border-dashed border-[#CBD5E1] bg-[#F8FAFC] px-5 py-8 text-center", className)}>
      <div className="type-card-title text-[#475569]">{title}</div>
      <div className="type-body-muted mt-2 text-[#94A3B8]">{description}</div>
      {action && <div className="mt-4 flex justify-center">{action}</div>}
    </div>
  );
}

export function SkeletonBlock({ className }: { className?: string }) {
  return <div className={cn("animate-pulse rounded-2xl bg-[#EAF0F6]", className)} />;
}

export function InlineBanner({
  title,
  description,
  tone = "info",
}: {
  title: string;
  description?: string;
  tone?: "info" | "success" | "warning" | "error";
}) {
  const toneClass = tone === "success"
    ? "border-[#BBF7D0] bg-[#F0FDF4] text-[#166534]"
    : tone === "warning"
      ? "border-[#FDE68A] bg-[#FFFBEB] text-[#B45309]"
      : tone === "error"
        ? "border-[#FECACA] bg-[#FEF2F2] text-[#991B1B]"
        : "border-[#BFDBFE] bg-[#EFF6FF] text-[#1D4ED8]";

  return (
    <div className={cn("rounded-2xl border px-4 py-3", toneClass)}>
      <div className="text-[14px] font-semibold tracking-[-0.01em]">{title}</div>
      {description && <div className="mt-1 text-[13px] leading-6 opacity-90">{description}</div>}
    </div>
  );
}
