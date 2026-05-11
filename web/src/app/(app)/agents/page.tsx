"use client";

import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Bot, Plus, Power, Settings2, Trash2, X } from "lucide-react";
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
  allowed_classification_max: "confidential",
  allow_code_context: true,
  allow_project_memory: true,
  allow_conversation_history: true,
  require_redaction: true,
  external_processing_allowed: true,
  retention_policy: "provider_default",
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
  // Full agent maintenance state. Replaces the old key-rotation modal so
  // the user can edit every meaningful field (name / model / Base URL /
  // role prompt / data policy / API key) from one place. Each field is
  // optional in the PATCH; empty strings are dropped before send so
  // existing values are preserved when the user only changes one thing.
  const [maintainingFor, setMaintainingFor] = useState<AgentProfile | null>(null);
  const [maintainForm, setMaintainForm] = useState({
    name: "",
    model: "",
    base_url: "",
    role_prompt: "",
    api_key: "",
    allowed_classification_max: "confidential",
    allow_code_context: true,
    allow_project_memory: true,
    allow_conversation_history: true,
    require_redaction: true,
    external_processing_allowed: true,
    retention_policy: "provider_default",
  });
  const [maintaining, setMaintaining] = useState(false);

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
        allowed_classification_max: form.allowed_classification_max,
        allow_code_context: form.allow_code_context,
        allow_project_memory: form.allow_project_memory,
        allow_conversation_history: form.allow_conversation_history,
        require_redaction: form.require_redaction,
        external_processing_allowed: form.external_processing_allowed,
        retention_policy: form.retention_policy,
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

  function openMaintain(profile: AgentProfile) {
    // Preload form with current values so the user sees what's there. The
    // API key field intentionally stays blank — the encrypted value never
    // round-trips through the UI; blank-on-submit means "keep current".
    setMaintainingFor(profile);
    setMaintainForm({
      name: profile.name,
      model: profile.model,
      base_url: profile.base_url ?? "",
      role_prompt: profile.role_prompt ?? "",
      api_key: "",
      allowed_classification_max: profile.allowed_classification_max,
      allow_code_context: profile.allow_code_context,
      allow_project_memory: profile.allow_project_memory,
      allow_conversation_history: profile.allow_conversation_history,
      require_redaction: profile.require_redaction,
      external_processing_allowed: profile.external_processing_allowed,
      retention_policy: profile.retention_policy,
    });
    setError("");
  }
  function closeMaintain() {
    setMaintainingFor(null);
    setError("");
  }
  async function submitMaintain(e: FormEvent) {
    e.preventDefault();
    if (!maintainingFor) return;
    setMaintaining(true);
    setError("");
    try {
      // Strip empty strings so the PATCH only carries deltas. The backend
      // treats omitted fields as "preserve existing" (see
      // api/agent_profiles.rs::UpdateAgentProfileRequest — every column
      // is Option<T>). The api_key is encrypted server-side with the same
      // TokenCipher used by other secret rotations.
      const patch: Parameters<typeof agentProfiles.update>[1] = {
        name: maintainForm.name.trim() || undefined,
        model: maintainForm.model.trim() || undefined,
        base_url: maintainForm.base_url.trim() || undefined,
        role_prompt: maintainForm.role_prompt.trim() || undefined,
        api_key: maintainForm.api_key.trim() || undefined,
        allowed_classification_max: maintainForm.allowed_classification_max,
        allow_code_context: maintainForm.allow_code_context,
        allow_project_memory: maintainForm.allow_project_memory,
        allow_conversation_history: maintainForm.allow_conversation_history,
        require_redaction: maintainForm.require_redaction,
        external_processing_allowed: maintainForm.external_processing_allowed,
        retention_policy: maintainForm.retention_policy,
      };
      const updated = await agentProfiles.update(maintainingFor.id, patch);
      setProfiles((items) => items.map((item) => item.id === updated.id ? updated : item));
      pushToast({
        tone: "success",
        title: t("agents.rotateSuccessTitle"),
        description: t("agents.rotateSuccessDesc").replace("{name}", updated.name),
      });
      closeMaintain();
    } catch (err) {
      const message = err instanceof Error ? err.message : t("agents.rotateFailedDesc");
      setError(message);
    } finally {
      setMaintaining(false);
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
            <div className="rounded-2xl border border-[#E2E8F0] bg-white p-4">
              <div className="text-sm font-semibold text-[#1A1A2E]">Data policy</div>
              <p className="mt-1 text-xs text-[#64748B]">Controls what this external/custom agent may receive after Context Firewall redaction.</p>
              <div className="mt-4 grid gap-4 md:grid-cols-2">
                <label className="space-y-1 text-sm font-medium text-[#334155]">
                  Max classification
                  <select value={form.allowed_classification_max} onChange={(e) => setForm((f) => ({ ...f, allowed_classification_max: e.target.value }))} className="h-11 w-full rounded-xl border border-[#D6DFEA] bg-white px-3 text-sm outline-none focus:border-[#0050A0]">
                    <option value="public">Public</option>
                    <option value="internal">Internal</option>
                    <option value="confidential">Confidential</option>
                    <option value="restricted">Restricted</option>
                    <option value="secret">Secret</option>
                  </select>
                </label>
                <label className="space-y-1 text-sm font-medium text-[#334155]">
                  Retention policy
                  <select value={form.retention_policy} onChange={(e) => setForm((f) => ({ ...f, retention_policy: e.target.value }))} className="h-11 w-full rounded-xl border border-[#D6DFEA] bg-white px-3 text-sm outline-none focus:border-[#0050A0]">
                    <option value="none">None / no retention requested</option>
                    <option value="session">Session only</option>
                    <option value="provider_default">Provider default</option>
                  </select>
                </label>
              </div>
              <div className="mt-4 grid gap-2 text-sm text-[#334155] md:grid-cols-2">
                <PolicyCheckbox label="Allow code context" checked={form.allow_code_context} onChange={(value) => setForm((f) => ({ ...f, allow_code_context: value }))} />
                <PolicyCheckbox label="Allow project memory" checked={form.allow_project_memory} onChange={(value) => setForm((f) => ({ ...f, allow_project_memory: value }))} />
                <PolicyCheckbox label="Allow conversation history" checked={form.allow_conversation_history} onChange={(value) => setForm((f) => ({ ...f, allow_conversation_history: value }))} />
                <PolicyCheckbox label="Require secret redaction" checked={form.require_redaction} onChange={(value) => setForm((f) => ({ ...f, require_redaction: value }))} />
                <PolicyCheckbox label="External processing allowed" checked={form.external_processing_allowed} onChange={(value) => setForm((f) => ({ ...f, external_processing_allowed: value }))} />
              </div>
            </div>
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
                <div className="mt-3 flex flex-wrap gap-2 text-xs text-[#64748B]">
                  <span className="rounded-full bg-[#F8FAFC] px-2.5 py-1">Max: {profile.allowed_classification_max}</span>
                  {!profile.allow_code_context && <span className="rounded-full bg-amber-50 px-2.5 py-1 text-amber-700">No code context</span>}
                  {!profile.allow_project_memory && <span className="rounded-full bg-amber-50 px-2.5 py-1 text-amber-700">No project memory</span>}
                  {!profile.allow_conversation_history && <span className="rounded-full bg-amber-50 px-2.5 py-1 text-amber-700">No history</span>}
                  {profile.require_redaction && <span className="rounded-full bg-emerald-50 px-2.5 py-1 text-emerald-700">Redaction required</span>}
                  <span className="rounded-full bg-[#F8FAFC] px-2.5 py-1">Retention: {profile.retention_policy}</span>
                </div>
                <p className="mt-3 text-xs text-[#94A3B8]">{t("agents.updatedLabel")} {formatDate(profile.updated_at)}</p>
              </div>
              {/* Tightened action toolbar. The old wrap-into-three-rows
                  cluster (停用 / 輪替金鑰 / 刪除 stacked vertically) was
                  visually noisy and pushed the card too wide. Now the
                  primary action is "維護" (which opens the comprehensive
                  edit modal), enable/disable is an inline toggle, and
                  delete is a tucked-away icon button.
                  Wraps onto a 2nd row only at narrow widths. */}
              <div className="flex flex-shrink-0 items-center gap-2 self-end lg:self-center">
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => void toggleEnabled(profile)}
                  title={profile.enabled ? t("agents.disable") : t("agents.enable")}
                >
                  <Power size={14} className={profile.enabled ? "text-emerald-600" : "text-[#94A3B8]"} />
                  <span className="hidden sm:inline">{profile.enabled ? t("agents.disable") : t("agents.enable")}</span>
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => openMaintain(profile)}
                  title={t("agents.rotateKey")}
                >
                  <Settings2 size={14} />
                  {t("agents.rotateKey")}
                </Button>
                <button
                  type="button"
                  onClick={() => void handleDelete(profile)}
                  title={t("common.delete")}
                  className="inline-flex h-9 w-9 items-center justify-center rounded-xl border border-[#E2E8F0] bg-white text-[#94A3B8] transition hover:border-red-200 hover:text-red-600"
                >
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          </Card>
        ))}
      </section>

      {maintainingFor && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/35 p-6 backdrop-blur-sm"
          onClick={closeMaintain}
        >
          <div
            className="flex max-h-[90vh] w-full max-w-2xl flex-col rounded-[24px] bg-white shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between border-b border-[#E2E8F0] px-6 py-4">
              <h3 className="type-card-title flex items-center gap-2">
                <Settings2 size={16} /> {t("agents.rotateKeyFor").replace("{name}", maintainingFor.name)}
              </h3>
              <button onClick={closeMaintain} className="rounded-xl p-1.5 text-[#64748B] transition hover:bg-[#F1F5F9] hover:text-[#1A1A2E]">
                <X size={16} />
              </button>
            </div>
            <form
              onSubmit={(e) => void submitMaintain(e)}
              className="flex-1 space-y-4 overflow-y-auto px-6 py-5"
            >
              <p className="type-body-muted">{t("agents.rotateHint")}</p>

              <div className="grid gap-4 md:grid-cols-2">
                <Input
                  id="maintain-name"
                  label={t("agents.displayName")}
                  value={maintainForm.name}
                  onChange={(e) => setMaintainForm((f) => ({ ...f, name: e.target.value }))}
                />
                <Input
                  id="maintain-model"
                  label={t("agents.model")}
                  value={maintainForm.model}
                  onChange={(e) => setMaintainForm((f) => ({ ...f, model: e.target.value }))}
                />
                <Input
                  id="maintain-base-url"
                  label={t("agents.baseUrl")}
                  placeholder={t("agents.baseUrlPlaceholder")}
                  value={maintainForm.base_url}
                  onChange={(e) => setMaintainForm((f) => ({ ...f, base_url: e.target.value }))}
                />
                <Input
                  id="maintain-key"
                  label={t("agents.newApiKey")}
                  type="password"
                  placeholder={t("agents.apiKeyPlaceholder")}
                  value={maintainForm.api_key}
                  onChange={(e) => setMaintainForm((f) => ({ ...f, api_key: e.target.value }))}
                  hint="Leave blank to keep the current key"
                />
              </div>

              <div className="rounded-2xl border border-[#E2E8F0] bg-white p-4">
                <div className="text-[14px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">Data policy</div>
                <p className="type-meta mt-1">
                  Controls what this agent may receive after Context Firewall redaction.
                </p>
                <div className="mt-4 grid gap-4 md:grid-cols-2">
                  <label className="space-y-1 text-[13px] font-medium text-[#334155]">
                    Max classification
                    <select
                      value={maintainForm.allowed_classification_max}
                      onChange={(e) => setMaintainForm((f) => ({ ...f, allowed_classification_max: e.target.value }))}
                      className="h-11 w-full rounded-xl border border-[#D6DFEA] bg-white px-3 text-[14px] outline-none focus:border-[#0050A0]"
                    >
                      <option value="public">Public</option>
                      <option value="internal">Internal</option>
                      <option value="confidential">Confidential</option>
                      <option value="restricted">Restricted</option>
                      <option value="secret">Secret</option>
                    </select>
                  </label>
                  <label className="space-y-1 text-[13px] font-medium text-[#334155]">
                    Retention policy
                    <select
                      value={maintainForm.retention_policy}
                      onChange={(e) => setMaintainForm((f) => ({ ...f, retention_policy: e.target.value }))}
                      className="h-11 w-full rounded-xl border border-[#D6DFEA] bg-white px-3 text-[14px] outline-none focus:border-[#0050A0]"
                    >
                      <option value="none">None / no retention requested</option>
                      <option value="session">Session only</option>
                      <option value="provider_default">Provider default</option>
                    </select>
                  </label>
                </div>
                <div className="mt-4 grid gap-2 text-[13px] text-[#334155] md:grid-cols-2">
                  <PolicyCheckbox
                    label="Allow code context"
                    checked={maintainForm.allow_code_context}
                    onChange={(value) => setMaintainForm((f) => ({ ...f, allow_code_context: value }))}
                  />
                  <PolicyCheckbox
                    label="Allow project memory"
                    checked={maintainForm.allow_project_memory}
                    onChange={(value) => setMaintainForm((f) => ({ ...f, allow_project_memory: value }))}
                  />
                  <PolicyCheckbox
                    label="Allow conversation history"
                    checked={maintainForm.allow_conversation_history}
                    onChange={(value) => setMaintainForm((f) => ({ ...f, allow_conversation_history: value }))}
                  />
                  <PolicyCheckbox
                    label="Require secret redaction"
                    checked={maintainForm.require_redaction}
                    onChange={(value) => setMaintainForm((f) => ({ ...f, require_redaction: value }))}
                  />
                  <PolicyCheckbox
                    label="External processing allowed"
                    checked={maintainForm.external_processing_allowed}
                    onChange={(value) => setMaintainForm((f) => ({ ...f, external_processing_allowed: value }))}
                  />
                </div>
              </div>

              <label className="block space-y-1 text-[13px] font-medium text-[#334155]">
                {t("agents.rolePrompt")}
                <textarea
                  value={maintainForm.role_prompt}
                  onChange={(e) => setMaintainForm((f) => ({ ...f, role_prompt: e.target.value }))}
                  placeholder={t("agents.rolePromptPlaceholder")}
                  className="min-h-32 w-full rounded-xl border border-[#D6DFEA] bg-white px-3 py-2 text-[14px] leading-7 outline-none focus:border-[#0050A0]"
                />
              </label>

              {error && <InlineBanner tone="error" title={t("agents.rotateFailedTitle")} description={error} />}
            </form>
            <div className="flex items-center justify-end gap-2 border-t border-[#E2E8F0] px-6 py-4">
              <Button type="button" variant="secondary" onClick={closeMaintain}>{t("common.cancel")}</Button>
              <Button
                type="button"
                loading={maintaining}
                onClick={(e) => void submitMaintain(e as unknown as FormEvent)}
              >
                {t("agents.rotateConfirm")}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function PolicyCheckbox({ label, checked, onChange }: { label: string; checked: boolean; onChange: (value: boolean) => void }) {
  return (
    <label className="flex items-center gap-2 rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-2">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} className="h-4 w-4 rounded border-[#CBD5E1]" />
      <span>{label}</span>
    </label>
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
