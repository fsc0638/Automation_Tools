"use client";

import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Bot, Plus, Trash2 } from "lucide-react";
import { agentProfiles, type AgentProfile } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, InlineBanner, SectionEmpty, SkeletonBlock } from "@/components/ui/card";
import { formatDate } from "@/lib/utils";
import { useToastStore } from "@/lib/toast-store";

const emptyForm = {
  name: "",
  provider: "openai",
  model: "gpt-4.1",
  base_url: "",
  role_prompt: "",
  api_key: "",
};

const providerHints: Record<string, string> = {
  openai: "Uses https://api.openai.com/v1/chat/completions by default.",
  openai_compatible: "For OpenAI-compatible gateways. Set base URL, e.g. https://host/v1.",
  gemini: "Uses Google Gemini generateContent. Model example: gemini-2.5-pro.",
  anthropic: "Uses Anthropic Messages API. Model example: claude-3-5-sonnet-latest.",
};

export default function AgentsPage() {
  const pushToast = useToastStore((state) => state.pushToast);
  const [profiles, setProfiles] = useState<AgentProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState(emptyForm);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

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
      pushToast({ tone: "success", title: "Agent saved", description: `${profile.name} is available in project chat.` });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to save agent";
      setError(message);
      pushToast({ tone: "error", title: "Agent creation failed", description: message });
    } finally {
      setSaving(false);
    }
  }

  async function toggleEnabled(profile: AgentProfile) {
    const updated = await agentProfiles.update(profile.id, { enabled: !profile.enabled });
    setProfiles((items) => items.map((item) => item.id === updated.id ? updated : item));
  }

  async function handleDelete(profile: AgentProfile) {
    if (!confirm(`Delete agent "${profile.name}"? The API key will be removed.`)) return;
    await agentProfiles.delete(profile.id);
    setProfiles((items) => items.filter((item) => item.id !== profile.id));
    pushToast({ tone: "warning", title: "Agent deleted", description: `${profile.name} was removed.` });
  }

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-6 p-8">
      <section className="rounded-[28px] border border-[#E2E8F0] bg-white p-6 shadow-sm">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <div className="inline-flex items-center gap-2 rounded-full bg-[#EEF4FF] px-3 py-1 text-xs font-semibold text-[#0050A0]">
              <Bot size={13} /> Agent profiles
            </div>
            <h1 className="mt-3 text-3xl font-semibold tracking-tight text-[#1A1A2E]">Agents</h1>
            <p className="mt-2 max-w-2xl text-sm text-[#64748B]">
              Add GPT, Gemini, Claude, or OpenAI-compatible agents with each user&apos;s own API key. Keys are encrypted server-side and never returned to the browser.
            </p>
          </div>
          <Button onClick={() => setShowCreate((value) => !value)}><Plus size={16} /> Add Agent</Button>
        </div>
        <div className="mt-6 grid gap-3 md:grid-cols-3">
          <Summary label="Total agents" value={profiles.length} />
          <Summary label="Enabled" value={enabledCount} />
          <Summary label="Providers" value={new Set(profiles.map((p) => p.provider)).size} />
        </div>
      </section>

      {showCreate && (
        <Card className="rounded-[24px] p-6 shadow-sm">
          <h2 className="text-lg font-semibold text-[#1A1A2E]">Add Agent</h2>
          <p className="mt-1 text-sm text-[#64748B]">Use a least-privilege model key. For Claude Code CLI style execution, keep it as patch-plan only until permission controls are added.</p>
          <form onSubmit={handleCreate} className="mt-5 space-y-4">
            <div className="grid gap-4 md:grid-cols-2">
              <Input id="agent-name" label="Display name" placeholder="GPT Architect" value={form.name} onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} required />
              <label className="space-y-1 text-sm font-medium text-[#334155]">
                Provider
                <select value={form.provider} onChange={(e) => setForm((f) => ({ ...f, provider: e.target.value }))} className="h-11 w-full rounded-xl border border-[#D6DFEA] bg-white px-3 text-sm outline-none focus:border-[#0050A0]">
                  <option value="openai">OpenAI</option>
                  <option value="openai_compatible">OpenAI-compatible</option>
                  <option value="gemini">Gemini</option>
                  <option value="anthropic">Anthropic / Claude</option>
                </select>
              </label>
              <Input id="agent-model" label="Model" placeholder="gpt-4.1 / gemini-2.5-pro / claude-..." value={form.model} onChange={(e) => setForm((f) => ({ ...f, model: e.target.value }))} required />
              <Input id="agent-base-url" label="Base URL / endpoint" placeholder="Optional for OpenAI/Gemini/Anthropic" value={form.base_url} onChange={(e) => setForm((f) => ({ ...f, base_url: e.target.value }))} />
              <Input id="agent-key" label="API Key" type="password" placeholder="Stored encrypted; never shown again" value={form.api_key} onChange={(e) => setForm((f) => ({ ...f, api_key: e.target.value }))} required />
            </div>
            <div className="rounded-2xl border border-[#E2E8F0] bg-[#F8FAFC] px-4 py-3 text-xs text-[#64748B]">{providerHints[form.provider]}</div>
            <label className="block space-y-1 text-sm font-medium text-[#334155]">
              Role prompt
              <textarea value={form.role_prompt} onChange={(e) => setForm((f) => ({ ...f, role_prompt: e.target.value }))} placeholder="Example: You are a security-focused reviewer. Prioritize concrete evidence and safe patches." className="min-h-28 w-full rounded-xl border border-[#D6DFEA] bg-white px-3 py-2 text-sm outline-none focus:border-[#0050A0]" />
            </label>
            {error && <InlineBanner tone="error" title="Agent could not be saved" description={error} />}
            <div className="flex gap-3">
              <Button type="submit" loading={saving}>Save Agent</Button>
              <Button type="button" variant="secondary" onClick={() => setShowCreate(false)}>Cancel</Button>
            </div>
          </form>
        </Card>
      )}

      <section className="grid gap-4">
        {loading ? (
          <SkeletonBlock className="h-36 rounded-[24px]" />
        ) : profiles.length === 0 ? (
          <SectionEmpty title="No custom agents yet" description="Add GPT, Gemini, Claude, or an OpenAI-compatible endpoint to use it from project chat." action={<Button onClick={() => setShowCreate(true)}><Plus size={14} /> Add Agent</Button>} />
        ) : profiles.map((profile) => (
          <Card key={profile.id} className="rounded-[24px] p-5 shadow-sm">
            <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="text-lg font-semibold text-[#1A1A2E]">{profile.name}</h2>
                  <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2.5 py-1 text-xs text-[#64748B]">{profile.provider}</span>
                  <span className={profile.enabled ? "rounded-full bg-emerald-50 px-2.5 py-1 text-xs text-emerald-700" : "rounded-full bg-slate-100 px-2.5 py-1 text-xs text-slate-500"}>{profile.enabled ? "Enabled" : "Disabled"}</span>
                </div>
                <p className="mt-1 text-sm text-[#64748B]">Model: <span className="font-medium text-[#334155]">{profile.model}</span></p>
                {profile.base_url && <p className="mt-1 truncate text-xs text-[#94A3B8]">Base URL: {profile.base_url}</p>}
                {profile.role_prompt && <p className="mt-3 line-clamp-2 text-sm text-[#475569]">{profile.role_prompt}</p>}
                <p className="mt-3 text-xs text-[#94A3B8]">Updated {formatDate(profile.updated_at)}</p>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button variant="secondary" onClick={() => void toggleEnabled(profile)}>{profile.enabled ? "Disable" : "Enable"}</Button>
                <Button variant="secondary" onClick={() => void handleDelete(profile)}><Trash2 size={14} /> Delete</Button>
              </div>
            </div>
          </Card>
        ))}
      </section>
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
