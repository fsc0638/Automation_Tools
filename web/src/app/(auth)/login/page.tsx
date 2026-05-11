"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { Bot, GitBranch, Sparkles } from "lucide-react";
import { auth } from "@/lib/api";
import { useAuthStore } from "@/lib/store";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";
import { LocaleSwitcher } from "@/components/LocaleSwitcher";

const productHighlights = [
  "Repository-aware conversations with file and branch context",
  "Multi-agent workflows across OpenClaw, Hermes, and Debate Mode",
  "A focused coding workspace instead of a generic admin dashboard",
];

export default function LoginPage() {
  const router = useRouter();
  const setAuth = useAuthStore((s) => s.setAuth);
  const t = useT();
  const [form, setForm] = useState({ email: "", password: "" });
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  // Render footer year on the client so a year roll-over between SSR and
  // hydration cannot produce a "text content did not match" mismatch.
  const [year, setYear] = useState<number | null>(null);
  useEffect(() => {
    setYear(new Date().getFullYear());
  }, []);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      const res = await auth.login(form);
      setAuth(res.access_token, res.user, res.refresh_token);
      router.push("/projects");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Login failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="grid min-h-screen bg-[#F5F7FB] lg:grid-cols-[1.08fr_0.92fr]">
      <section className="relative hidden overflow-hidden bg-[radial-gradient(circle_at_top,_rgba(58,126,204,0.26),_transparent_34%),linear-gradient(180deg,#061936_0%,#08142B_100%)] px-10 py-12 text-white lg:flex lg:flex-col lg:justify-between xl:px-16">
        <div>
          <div className="inline-flex items-center gap-3 rounded-full border border-white/10 bg-white/7 px-4 py-2 text-[14px] backdrop-blur-sm">
            <div className="flex h-8 w-8 items-center justify-center rounded-2xl bg-white/10 font-bold">K</div>
            <span className="font-semibold tracking-[-0.01em]">Kway Dev Workspace</span>
          </div>
          <div className="mt-10 max-w-xl">
            <div className="inline-flex items-center gap-2 rounded-full bg-[#3A7ECC]/18 px-3 py-1 text-[12px] font-semibold tracking-[0.08em] text-blue-100">
              <Sparkles size={13} /> AI-native developer workspace
            </div>
            <h1 className="mt-5 text-[2.5rem] font-semibold leading-[1.12] tracking-[-0.03em]">
              Sign in to a repo-aware workspace built for multi-agent coding.
            </h1>
            <p className="mt-5 max-w-lg text-[15px] leading-8 text-blue-100/78">
              Keep repository context, branch status, files, and collaborative agent conversations visible in the same place.
            </p>
          </div>

          <div className="mt-10 space-y-4">
            {productHighlights.map((item, index) => (
              <div key={item} className="flex items-start gap-3 rounded-2xl border border-white/10 bg-white/6 p-4 backdrop-blur-sm">
                <div className="mt-0.5 flex h-7 w-7 items-center justify-center rounded-xl bg-white/10 text-[13px] font-semibold">
                  0{index + 1}
                </div>
                <p className="text-[14px] leading-7 text-blue-100/82">{item}</p>
              </div>
            ))}
          </div>
        </div>

        <div className="grid grid-cols-3 gap-3 text-sm">
          <MetricCard icon={<Bot size={15} />} label="Agents" value="3 modes" />
          <MetricCard icon={<GitBranch size={15} />} label="Git context" value="Live branch state" />
          <MetricCard icon={<Sparkles size={15} />} label="Workspace" value="Project-aware UI" />
        </div>
      </section>

      <section className="flex items-center justify-center px-6 py-10 sm:px-10">
        <div className="w-full max-w-md">
          <div className="mb-8 text-center lg:hidden">
            <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl bg-[#002D62] text-lg font-bold text-white shadow-sm">K</div>
            <h1 className="mt-4 text-[1.9rem] font-semibold tracking-[-0.03em] text-[#1A1A2E]">Kway Dev</h1>
            <p className="mt-2 text-[14px] leading-6 text-[#64748B]">AI-native developer workspace</p>
          </div>

          <div className="rounded-[28px] border border-[#E2E8F0] bg-white p-8 shadow-[0_18px_50px_rgba(15,23,42,0.08)]">
            <div className="mb-6 flex items-start justify-between gap-3">
              <div>
                <div className="text-[14px] font-medium text-[#0050A0]">{t("auth.welcomeBack")}</div>
                <h2 className="mt-2 text-[1.9rem] font-semibold tracking-[-0.03em] text-[#1A1A2E]">{t("common.signIn")}</h2>
                <p className="mt-2 text-[15px] leading-7 text-[#64748B]">
                  {t("auth.signInToContinue")}
                </p>
              </div>
              <LocaleSwitcher tone="light" />
            </div>

            <form onSubmit={handleSubmit} className="flex flex-col gap-4">
              <Input
                id="email"
                label={t("auth.email")}
                type="email"
                placeholder="you@example.com"
                value={form.email}
                onChange={(e) => setForm((current) => ({ ...current, email: e.target.value }))}
                required
              />
              <Input
                id="password"
                label={t("auth.password")}
                type="password"
                placeholder="••••••••"
                value={form.password}
                onChange={(e) => setForm((current) => ({ ...current, password: e.target.value }))}
                required
              />

              {error && (
                <p className="rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-[14px] leading-6 text-[#C8102E]">
                  {error}
                </p>
              )}

              <Button type="submit" size="lg" loading={loading} className="mt-2 w-full">
                {loading ? t("auth.signingIn") : t("common.signIn")}
              </Button>
            </form>

            <p className="mt-6 text-center text-[14px] leading-6 text-[#64748B]">
              {t("auth.dontHaveAccount")}{" "}
              <Link href="/register" className="font-medium text-[#0050A0] hover:underline">
                {t("auth.signUp")}
              </Link>
            </p>
          </div>

          <p className="mt-6 text-center text-[12px] leading-5 text-[#94A3B8]" suppressHydrationWarning>
            © {year ?? ""} Kway In-house Dev Platform
          </p>
        </div>
      </section>
    </div>
  );
}

function MetricCard({ icon, label, value }: { icon: ReactNode; label: string; value: string }) {
  return (
    <div className="rounded-2xl border border-white/10 bg-white/7 p-4 backdrop-blur-sm">
      <div className="text-blue-100/70">{icon}</div>
      <div className="mt-3 text-[12px] tracking-[0.08em] text-blue-100/45">{label}</div>
      <div className="mt-1 text-[14px] font-medium tracking-[-0.01em] text-white">{value}</div>
    </div>
  );
}
