"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState, type FormEvent, type ReactNode } from "react";
import { Bot, GitBranch, Sparkles } from "lucide-react";
import { auth } from "@/lib/api";
import { useAuthStore } from "@/lib/store";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";
import { LocaleSwitcher } from "@/components/LocaleSwitcher";

const productHighlights = [
  "Create a workspace that understands project structure and repository state",
  "Reuse Git profiles to connect private repositories across projects",
  "Switch between fast answers, deeper reasoning, and debate-style analysis",
];

export default function RegisterPage() {
  const router = useRouter();
  const setAuth = useAuthStore((s) => s.setAuth);
  const t = useT();
  const [form, setForm] = useState({ email: "", password: "", display_name: "" });
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      const res = await auth.register(form);
      setAuth(res.access_token, res.user);
      router.push("/projects");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Registration failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="grid min-h-screen bg-[#F5F7FB] lg:grid-cols-[1.05fr_0.95fr]">
      <section className="relative hidden overflow-hidden bg-[radial-gradient(circle_at_top,_rgba(124,58,237,0.18),_transparent_34%),linear-gradient(180deg,#08142B_0%,#061936_100%)] px-10 py-12 text-white lg:flex lg:flex-col lg:justify-between xl:px-16">
        <div>
          <div className="inline-flex items-center gap-3 rounded-full border border-white/10 bg-white/7 px-4 py-2 text-sm backdrop-blur-sm">
            <div className="flex h-8 w-8 items-center justify-center rounded-2xl bg-white/10 font-bold">K</div>
            <span className="font-semibold tracking-wide">Kway Dev Workspace</span>
          </div>
          <div className="mt-10 max-w-xl">
            <div className="inline-flex items-center gap-2 rounded-full bg-white/10 px-3 py-1 text-xs font-semibold uppercase tracking-[0.16em] text-blue-100">
              <Sparkles size={13} /> Build your workspace
            </div>
            <h1 className="mt-5 text-4xl font-semibold leading-tight">
              Create an account for a coding workspace designed around project context.
            </h1>
            <p className="mt-5 text-base leading-8 text-blue-100/75">
              Connect repositories, inspect files, and run multi-agent conversations inside a calmer, more structured developer experience.
            </p>
          </div>

          <div className="mt-10 space-y-4">
            {productHighlights.map((item, index) => (
              <div key={item} className="flex items-start gap-3 rounded-2xl border border-white/10 bg-white/6 p-4 backdrop-blur-sm">
                <div className="mt-0.5 flex h-7 w-7 items-center justify-center rounded-xl bg-white/10 text-sm font-semibold">
                  0{index + 1}
                </div>
                <p className="text-sm leading-6 text-blue-100/80">{item}</p>
              </div>
            ))}
          </div>
        </div>

        <div className="grid grid-cols-3 gap-3 text-sm">
          <MetricCard icon={<Bot size={15} />} label="Agents" value="Hermes + OpenClaw" />
          <MetricCard icon={<GitBranch size={15} />} label="Projects" value="Repo-linked setup" />
          <MetricCard icon={<Sparkles size={15} />} label="Experience" value="Modern workspace UI" />
        </div>
      </section>

      <section className="flex items-center justify-center px-6 py-10 sm:px-10">
        <div className="w-full max-w-md">
          <div className="mb-8 text-center lg:hidden">
            <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl bg-[#002D62] text-lg font-bold text-white shadow-sm">K</div>
            <h1 className="mt-4 text-2xl font-semibold text-[#1A1A2E]">Create your Kway Dev account</h1>
            <p className="mt-2 text-sm text-[#64748B]">Start building inside an AI-native developer workspace</p>
          </div>

          <div className="rounded-[28px] border border-[#E2E8F0] bg-white p-8 shadow-[0_18px_50px_rgba(15,23,42,0.08)]">
            <div className="mb-6 flex items-start justify-between gap-3">
              <div>
                <div className="text-sm font-medium text-[#0050A0]">{t("auth.startWorkspace")}</div>
                <h2 className="mt-2 text-2xl font-semibold text-[#1A1A2E]">{t("auth.createAccount")}</h2>
              </div>
              <LocaleSwitcher tone="light" />
            </div>

            <form onSubmit={handleSubmit} className="flex flex-col gap-4">
              <Input
                id="name"
                label={t("auth.displayName")}
                placeholder="Your name"
                value={form.display_name}
                onChange={(e) => setForm((current) => ({ ...current, display_name: e.target.value }))}
                required
              />
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
                placeholder="Min 8 characters"
                value={form.password}
                onChange={(e) => setForm((current) => ({ ...current, password: e.target.value }))}
                required
              />

              {error && (
                <p className="rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-sm text-[#C8102E]">
                  {error}
                </p>
              )}

              <Button type="submit" size="lg" loading={loading} className="mt-2 w-full">
                {loading ? t("auth.signingUp") : t("auth.createAccount")}
              </Button>
            </form>

            <p className="mt-6 text-center text-sm text-[#64748B]">
              {t("auth.haveAccount")}{" "}
              <Link href="/login" className="font-medium text-[#0050A0] hover:underline">
                {t("common.signIn")}
              </Link>
            </p>
          </div>
        </div>
      </section>
    </div>
  );
}

function MetricCard({ icon, label, value }: { icon: ReactNode; label: string; value: string }) {
  return (
    <div className="rounded-2xl border border-white/10 bg-white/7 p-4 backdrop-blur-sm">
      <div className="text-blue-100/70">{icon}</div>
      <div className="mt-3 text-xs uppercase tracking-[0.12em] text-blue-100/45">{label}</div>
      <div className="mt-1 text-sm font-medium text-white">{value}</div>
    </div>
  );
}
