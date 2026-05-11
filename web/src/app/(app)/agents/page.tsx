"use client";

import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Bot, KeyRound, Plus, Trash2, X } from "lucide-react";
import { agentProfiles, type AgentProfile } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, InlineBanner, SectionEmpty, SkeletonBlock } from "@/components/ui/card";
import { formatDate } from "@/lib/utils";
import { useToastStore } from "@/lib/toast-store";
import { useT } from "@/lib/i18n";

const emptyForm = {
  name: "",
  provider: "openai",
  model: "gpt-4.1",
  base_url: "",
  role_prompt: "",
  api_key: "",
};

export default function AgentsPage() {
  const pushToast = useToastStore((state) => state.pushToast);
  const t = useT();
  const [profiles, setProfiles] = useState<AgentProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState(emptyForm);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [rotatingFor, setRotatingFor] = useState<AgentProfile | null>(null);
  const [rotateInput, setRotateInput] = useState("");
  const [rotating, setRotating] = useState(false);

  // Provider hints are translated at render time so they stay in sync
  // with the active locale (a const map outside the component would be
  // frozen at module-eval time).
  const providerHints: Record<string, string> = {
    openai: t("agents.providerHintOpenai"),
    openai_compatible: t("agents.providerHintCompat"),
    gemini: t("agents.providerHintGemini"),
    anthropic: t("agents.providerHintAnthropic"),
  };

  async function load() {
    setLoading(true);
    try {
      setProfiles(await agentProfiles.list());
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    const timer = window.setTimeout(() => { void load(); }, 0);
    return () => window.clearTimeout(timer);
  }, []);

  const enabledCount = useMemo(() => profiles.filter((p) => p.enabled).length, [profiles]);

  async function handleCreate(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setError("");
    try {
      const profile = await agentProfiles.create({
        name: form.name,
        provider: form.provider,
        model: form.model,
        base_url: form.base_url || undefined,
        role_prompt: form.role_prompt || undefined,
        api_key: form.api_key,
        enabled: true,
      });
      setProfiles((items) => [profile, ...items]);
      setForm(emptyForm);
      setShowCreate(false);
      pushToast({
        tone: "success",
        title: t("agents.savedTitle"),
        description: t("agents.savedDesc").replace("{name}", profile.name),
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : t("agents.createFailedDesc");
      setError(message);
      pushToast({ tone: "error", title: t("agents.createFailedTitle"), description: message });
    } finally {
      setSaving(false);
    }
  }

  function openRotate(profile: AgentProfile) {
    setRotatingFor(profile);
    setRotateInput("");
  }
  function closeRotate() {
    setRotatingFor(null);
    setRotateInput("");
  }
  async function submitRotate(e: FormEvent) {
    e.preventDefault();
    if (!rotatingFor) return;
    const next = rotateInput.trim();
    if (!next) return;
    setRotating(true);
    setError("");
    try {
      // Only api_key is sent — everything else server-side stays as-is
      // because the backend PATCH falls back to existing fields when the
      // request body omits them. The new key is encrypted with the same
      // TokenCipher used elsewhere.
      const updated = await agentProfiles.update(rotatingFor.id, { api_key: next });
      setProfiles((items) => items.map((item) => item.id === updated.id ? updated : item));
      pushToast({
        tone: "success",
        title: t("agents.rotateSuccessTitle"),
        description: t("agents.rotateSuccessDesc").replace("{name}", updated.name),
      });
      closeRotate();
    } catch (err) {
      const message = err instanceof Error ? err.message : t("agents.rotateFailedDesc");
      setError(message);
    } finally {
      setRotating(false);
    }
  }

  async function toggleEnabled(profile: AgentProfile) {
    const updated = await agentProfiles.update(profile.id, { enabled: !profile.enabled });
    setProfiles((items) => items.map((item) => item.id === updated.id ? updated : item));
  }

  async function handleDelete(profile: AgentProfile) {
    if (!confirm(t("agents.deleteConfirm").replace("{name}", profile.name))) return;
    await agentProfiles.delete(profile.id);
    setProfiles((items) => items.filter((item) => item.id !== profile.id));
    pushToast({
      tone: "warning",
      title: t("agents.deletedTitle"),
      description: t("agents.deletedDesc").replace("{name}", profile.name),
    });
  }

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-6 p-8">
      <section className="rounded-[28px] border border-[#E2E8F0] bg-white p-6 shadow-sm">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <div className="inline-flex items-center gap-2 rounded-full bg-[#EEF4FF] px-3 py-1 text-xs font-semibold text-[#0050A0]">
              <Bot size={13} /> {t("agents.headerPill")}
            </div>
            <h1 className="mt-3 text-3xl font-semibold tracking-tight text-[#1A1A2E]">{t("agents.pageTitle")}</h1>
            <p className="mt-2 max-w-2xl text-sm text-[#64748B]">{t("agents.pageDesc")}</p>
          </div>
          <Button onClick={() => setShowCreate((value) => !value)}><Plus size={16} /> {t("agents.addAgent")}</Button>
        </div>
        <div className="mt-6 grid gap-3 md:grid-cols-3">
          <Summary label={t("agents.totalAgents")} value={profiles.length} />
          <Summary label={t("agents.enabledCount")} value={enabledCount} />
          <Summary label={t("agents.providers")} value={new Set(profiles.map((p) => p.provider)).size} />
        </div>
      </section>

      {showCreate && (
        <Card className="rounded-[24px] p-6 shadow-sm">
          <h2 className="text-lg font-semibold text-[#1A1A2E]">{t("agents.addAgent")}</h2>
          <p className="mt-1 text-sm text-[#64748B]">{t("agents.addHint")}</p>
          <form onSubmit={handleCreate} className="mt-5 space-y-4">
            <div className="grid gap-4 md:grid-cols-2">
              <Input id="agent-name" label={t("agents.displayName")} placeholder="GPT Architect" value={form.name} onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} required />
              <label className="space-y-1 text-sm font-medium text-[#334155]">
                {t("agents.provider")}
                <select value={form.provider} onChange={(e) => setForm((f) => ({ ...f, provider: e.target.value }))} className="h-11 w-full rounded-xl border border-[#D6DFEA] bg-white px-3 text-sm outline-none focus:border-[#0050A0]">
                  <option value="openai">OpenAI</option>
                  <option value="openai_compatible">OpenAI-compatible</option>
                  <option value="gemini">Gemini</option>
                  <option value="anthropic">Anthropic / Claude</option>
                </select>
              </label>
              <Input id="agent-model" label={t("agents.model")} placeholder="gpt-4.1 / gemini-2.5-pro / claude-…" value={form.model} onChange={(e) => setForm((f) => ({ ...f, model: e.target.value }))} required />
              <Input id="agent-base-url" label={t("agents.baseUrl")} placeholder={t("agents.baseUrlPlaceholder")} value={form.base_url} onChange={(e) => setForm((f) => ({ ...f, base_url: e.target.value }))} />
              <Input id="agent-key" label={t("agents.apiKey")} type="password" placeholder={t("agents.apiKeyPlaceholder")} value={form.api_key} onChange={(e) => setForm((f) => ({ ...f, api_key: e.target.value }))} required />
            </div>
            <div className="rounded-2xl border border-[#E2E8F0] bg-[#F8FAFC] px-4 py-3 text-xs text-[#64748B]">{providerHints[form.provider]}</div>
            <label className="block space-y-1 text-sm font-medium text-[#334155]">
              {t("agents.rolePrompt")}
              <textarea value={form.role_prompt} onChange={(e) => setForm((f) => ({ ...f, role_prompt: e.target.value }))} placeholder={t("agents.rolePromptPlaceholder")} className="min-h-28 w-full rounded-xl border border-[#D6DFEA] bg-white px-3 py-2 text-sm outline-none focus:border-[#0050A0]" />
            </label>
            {error && <InlineBanner tone="error" title={t("agents.createFailedTitle")} description={error} />}
            <div className="flex gap-3">
              <Button type="submit" loading={saving}>{t("agents.save")}</Button>
              <Button type="button" variant="secondary" onClick={() => setShowCreate(false)}>{t("common.cancel")}</Button>
            </div>
          </form>
        </Card>
      )}

      <section className="grid gap-4">
        {loading ? (
          <SkeletonBlock className="h-36 rounded-[24px]" />
        ) : profiles.length === 0 ? (
          <SectionEmpty title={t("agents.emptyTitle")} description={t("agents.emptyDesc")} action={<Button onClick={() => setShowCreate(true)}><Plus size={14} /> {t("agents.addAgent")}</Button>} />
        ) : profiles.map((profile) => (
          <Card key={profile.id} className="rounded-[24px] p-5 shadow-sm">
            <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="text-lg font-semibold text-[#1A1A2E]">{profile.name}</h2>
                  <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2.5 py-1 text-xs text-[#64748B]">{profile.provider}</span>
                  <span className={profile.enabled ? "rounded-full bg-emerald-50 px-2.5 py-1 text-xs text-emerald-700" : "rounded-full bg-slate-100 px-2.5 py-1 text-xs text-slate-500"}>
                    {profile.enabled ? t("agents.enabled") : t("agents.disabled")}
                  </span>
                </div>
                <p className="mt-1 text-sm text-[#64748B]">{t("agents.modelLabel")}: <span className="font-medium text-[#334155]">{profile.model}</span></p>
                {profile.base_url && <p className="mt-1 truncate text-xs text-[#94A3B8]">{t("agents.baseUrl")}: {profile.base_url}</p>}
                {profile.role_prompt && <p className="mt-3 line-clamp-2 text-sm text-[#475569]">{profile.role_prompt}</p>}
                <p className="mt-3 text-xs text-[#94A3B8]">{t("agents.updatedLabel")} {formatDate(profile.updated_at)}</p>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button variant="secondary" onClick={() => void toggleEnabled(profile)}>
                  {profile.enabled ? t("agents.disable") : t("agents.enable")}
                </Button>
                <Button variant="secondary" onClick={() => openRotate(profile)}>
                  <KeyRound size={14} /> {t("agents.rotateKey")}
                </Button>
                <Button variant="secondary" onClick={() => void handleDelete(profile)}><Trash2 size={14} /> {t("common.delete")}</Button>
              </div>
            </div>
          </Card>
        ))}
      </section>

      {rotatingFor && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-6" onClick={closeRotate}>
          <div className="w-full max-w-md rounded-lg bg-white shadow-2xl" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center justify-between border-b border-[#E2E8F0] px-5 py-3">
              <h3 className="text-sm font-semibold text-[#1A1A2E]">
                <KeyRound size={14} className="inline mr-1" /> {t("agents.rotateKeyFor").replace("{name}", rotatingFor.name)}
              </h3>
              <button onClick={closeRotate} className="rounded-md p-1 text-[#64748B] hover:bg-[#F1F5F9]"><X size={16} /></button>
            </div>
            <form onSubmit={(e) => void submitRotate(e)} className="space-y-4 px-5 py-4">
              <p className="text-xs text-[#64748B]">{t("agents.rotateHint")}</p>
              <Input
                id="rotate-key"
                label={t("agents.newApiKey")}
                type="password"
                placeholder={t("agents.apiKeyPlaceholder")}
                value={rotateInput}
                onChange={(e) => setRotateInput(e.target.value)}
                required
                autoFocus
              />
              {error && <InlineBanner tone="error" title={t("agents.rotateFailedTitle")} description={error} />}
              <div className="flex justify-end gap-2">
                <Button type="button" variant="secondary" onClick={closeRotate}>{t("common.cancel")}</Button>
                <Button type="submit" loading={rotating} disabled={!rotateInput.trim()}>{t("agents.rotateConfirm")}</Button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}

function Summary({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-[#F8FAFC] p-4">
      <div className="text-xs font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">{label}</div>
      <div className="mt-2 text-2xl font-semibold text-[#1A1A2E]">{value}</div>
    </div>
  );
}
