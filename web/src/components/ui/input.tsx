import { cn } from "@/lib/utils";
import { forwardRef } from "react";

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ className, label, error, id, ...props }, ref) => {
    return (
      <div className="flex flex-col gap-1.5">
        {label && (
          <label htmlFor={id} className="text-sm font-medium text-[#1A1A2E]">
            {label}
          </label>
        )}
        <input
          ref={ref}
          id={id}
          className={cn(
            "h-10 w-full rounded-lg border border-[#E2E8F0] bg-white px-3 text-sm text-[#1A1A2E] placeholder:text-[#94A3B8]",
            "focus:outline-none focus:ring-2 focus:ring-[#0050A0] focus:border-transparent",
            "disabled:opacity-50 disabled:cursor-not-allowed",
            error && "border-[#C8102E] focus:ring-[#C8102E]",
            className
          )}
          {...props}
        />
        {error && <p className="text-xs text-[#C8102E]">{error}</p>}
      </div>
    );
  }
);
Input.displayName = "Input";
