import { forwardRef, type InputHTMLAttributes } from "react";
import { cn } from "@/lib/utils";

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string;
  hint?: string;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ className, label, error, hint, id, ...props }, ref) => {
    return (
      <div className="flex flex-col gap-2">
        {label && (
          <label htmlFor={id} className="type-meta font-semibold text-[#334155]">
            {label}
          </label>
        )}
        <input
          ref={ref}
          id={id}
          className={cn(
            "h-11 w-full rounded-xl border border-[#D6DFEA] bg-white px-3.5 text-[15px] leading-6 text-[#1A1A2E] placeholder:text-[#94A3B8] shadow-[0_1px_2px_rgba(15,23,42,0.03)]",
            "focus:border-transparent focus:outline-none focus:ring-2 focus:ring-[#0050A0]",
            "disabled:cursor-not-allowed disabled:opacity-50",
            error && "border-[#C8102E] focus:ring-[#C8102E]",
            className
          )}
          {...props}
        />
        {error ? <p className="type-meta text-[#C8102E]">{error}</p> : hint ? <p className="type-meta text-[#94A3B8]">{hint}</p> : null}
      </div>
    );
  }
);
Input.displayName = "Input";
