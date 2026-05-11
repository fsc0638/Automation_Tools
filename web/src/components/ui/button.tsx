"use client";
import { forwardRef, type ButtonHTMLAttributes } from "react";
import { cn } from "@/lib/utils";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "ghost" | "danger" | "tonal" | "subtle";
  size?: "sm" | "md" | "lg";
  loading?: boolean;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "primary", size = "md", loading, children, disabled, ...props }, ref) => {
    return (
      <button
        ref={ref}
        disabled={disabled || loading}
        className={cn(
          "inline-flex items-center justify-center gap-2 rounded-xl font-medium tracking-[-0.01em] transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50",
          "shadow-[0_1px_2px_rgba(15,23,42,0.03)]",
          {
            "bg-[#0050A0] text-white hover:bg-[#003d7a] focus-visible:ring-[#0050A0]": variant === "primary",
            "border border-[#D6DFEA] bg-white text-[#1A1A2E] hover:border-[#94A3B8] hover:bg-[#F8FAFC] focus-visible:ring-[#0050A0]": variant === "secondary",
            "text-[#64748B] hover:bg-[#F1F5F9] hover:text-[#1A1A2E] focus-visible:ring-[#0050A0] shadow-none": variant === "ghost",
            "bg-[#C8102E] text-white hover:bg-[#a00e25] focus-visible:ring-[#C8102E]": variant === "danger",
            "border border-[#BFDBFE] bg-[#EFF6FF] text-[#0050A0] hover:bg-[#DBEAFE] focus-visible:ring-[#0050A0]": variant === "tonal",
            "bg-[#F8FAFC] text-[#475569] hover:bg-[#F1F5F9] hover:text-[#1A1A2E] focus-visible:ring-[#0050A0] shadow-none": variant === "subtle",
          },
          {
            "h-9 px-3.5 text-[13px]": size === "sm",
            "h-10 px-4 text-[14px]": size === "md",
            "h-11 px-6 text-[15px]": size === "lg",
          },
          className
        )}
        {...props}
      >
        {loading && (
          <span className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
        )}
        {children}
      </button>
    );
  }
);
Button.displayName = "Button";
