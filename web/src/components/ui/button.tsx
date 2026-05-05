import { cn } from "@/lib/utils";
import { forwardRef } from "react";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "ghost" | "danger";
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
          "inline-flex items-center justify-center gap-2 rounded-lg font-medium transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 disabled:opacity-50 disabled:cursor-not-allowed",
          {
            "bg-[#0050A0] text-white hover:bg-[#003d7a] focus-visible:ring-[#0050A0]": variant === "primary",
            "bg-white text-[#1A1A2E] border border-[#E2E8F0] hover:bg-[#F8F9FA] focus-visible:ring-[#0050A0]": variant === "secondary",
            "text-[#64748B] hover:bg-[#F1F5F9] hover:text-[#1A1A2E] focus-visible:ring-[#0050A0]": variant === "ghost",
            "bg-[#C8102E] text-white hover:bg-[#a00e25] focus-visible:ring-[#C8102E]": variant === "danger",
          },
          {
            "h-8 px-3 text-sm": size === "sm",
            "h-10 px-4 text-sm": size === "md",
            "h-11 px-6 text-base": size === "lg",
          },
          className
        )}
        {...props}
      >
        {loading && (
          <span className="h-4 w-4 border-2 border-current border-t-transparent rounded-full animate-spin" />
        )}
        {children}
      </button>
    );
  }
);
Button.displayName = "Button";
