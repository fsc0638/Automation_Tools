"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import { Bot, Eye, EyeOff, KeyRound, Lock, Pencil, Plus, Trash2 } from "lucide-react";
import { auth as authApi, vault as vaultApi, type VaultSecret } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Card,
  CardContent,
  CardHeader,
  InlineBanner,
  SectionEmpty,
  SkeletonBlock,
} from "@/components/ui/card";
import { useToastStore } from "@/lib/toast-store";
import { useT } from "@/lib/i18n";
import { formatDate } from "@/lib/utils";

// Auto-clear revealed value after this many seconds
const REVEAL_TTL_S = 30;

interface RevealState {
  id: string;
  value: string;
  secondsLeft: number;
}

const EMPTY_FORM = {
  label: "",
  secret_type: "api_key",
  username: "",
  url: "",
  note: "",
  ai_description: "",
  secret_value: "",
};

const EMPTY_PW_FORM = {
  current_password: "",
  new_password: "",
  confirm_new_password: "",
};

export default function VaultPage() {
  const pushToast = useToastStore((s) => s.pushToast);
  const t = useT();

  const [secrets, setSecrets] = useState<VaultSecret[]>([]);
  const [loading, setLoading] = useState(true);
  const [sessionExpired, setSessionExpired] = useState(false);

  // ── Create ─────────────────────────────────────────────────────────────
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState(EMPTY_FORM);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState("");

  // ── Delete ─────────────────────────────────────────────────────────────
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  // ── Reveal ─────────────────────────────────────────────────────────────
  const [revealState, setRevealState] = useState<RevealState | null>(null);
  const [revealing, setRevealing] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // ── Inline AI description edit ─────────────────────────────────────────
  const [editAiTarget, setEditAiTarget] = useState<string | null>(null);
  const [editAiValue, setEditAiValue] = useState("");
  const [savingAi, setSavingAi] = useState(false);

  // ── Change password ────────────────────────────────────────────────────
  const [showChangePw, setShowChangePw] = useState(false);
  const [pwForm, setPwForm] = useState(EMPTY_PW_FORM);
  const [changingPw, setChangingPw] = useState(false);
  const [pwError, setPwError] = useState("");

  // ── Load ───────────────────────────────────────────────────────────────
  const loadSecrets = useCallback(async () => {
    setLoading(true);
    setSessionExpired(false);
    try {
      const data = await vaultApi.list();
      setSecrets(data);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes("401") || /vault session/i.test(msg)) {
        setSessionExpired(true);
      }
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void loadSecrets(); }, [loadSecrets]);

  // ── Countdown ticker ───────────────────────────────────────────────────
  // Only (re)start the interval when the revealed secret ID changes so we
  // don't thrash the interval on every render.
  const revealId = revealState?.id;
  useEffect(() => {
    if (!revealId) return;
    if (timerRef.current) clearInterval(timerRef.current);
    timerRef.current = setInterval(() => {
      setRevealState((prev) => {
        if (!prev) return null;
        const next = prev.secondsLeft - 1;
        if (next <= 0) {
          clearInterval(timerRef.current!);
          timerRef.current = null;
          return null;
        }
        return { ...prev, secondsLeft: next };
      });
    }, 1000);
    return () => {
      if (timerRef.current) { clearInterval(timerRef.current); timerRef.current = null; }
    };
  }, [revealId]);

  // ── Handlers ───────────────────────────────────────────────────────────
  async function handleReveal(id: string) {
    // Toggle off if already revealed
    if (revealState?.id === id) {
      setRevealState(null);
      return;
    }
    setRevealing(id);
    try {
      const res = await vaultApi.reveal(id);
      setRevealState({ id, value: res.secret_value, secondsLeft: REVEAL_TTL_S });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes("401") || /vault session/i.test(msg)) {
        setSessionExpired(true);
      } else {
        pushToast({ title: t("vault.revealFailed"), description: msg, tone: "error" });
      }
    } finally {
      setRevealing(null);
    }
  }

  async function handleCreate(e: React.FormEvent) {
    e.preventDefault();
    setCreating(true);
    setCreateError("");
    try {
      const created = await vaultApi.create({
        label: form.label.trim(),
        secret_type: form.secret_type.trim() || undefined,
        username: form.username.trim() || undefined,
        url: form.url.trim() || undefined,
        note: form.note.trim() || undefined,
        ai_description: form.ai_description.trim() || undefined,
        secret_value: form.secret_value,
      });
      setSecrets((prev) => [created, ...prev]);
      setForm(EMPTY_FORM);
      setShowCreate(false);
      pushToast({ title: t("vault.created"), tone: "success" });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes("401") || /vault session/i.test(msg)) {
        setSessionExpired(true);
        setShowCreate(false);
      } else {
        setCreateError(msg);
      }
    } finally {
      setCreating(false);
    }
  }

  async function handleSaveAiDescription(id: string) {
    setSavingAi(true);
    try {
      const updated = await vaultApi.update(id, { ai_description: editAiValue.trim() });
      setSecrets((prev) => prev.map((s) => (s.id === id ? updated : s)));
      setEditAiTarget(null);
      pushToast({ title: t("vault.updated"), tone: "success" });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      pushToast({ title: t("common.error"), description: msg, tone: "error" });
    } finally {
      setSavingAi(false);
    }
  }

  async function handleDelete(id: string) {
    setDeleting(true);
    try {
      await vaultApi.delete(id);
      setSecrets((prev) => prev.filter((s) => s.id !== id));
      if (revealState?.id === id) setRevealState(null);
      setDeleteTarget(null);
      pushToast({ title: t("vault.deleted"), tone: "success" });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      pushToast({ title: t("common.error"), description: msg, tone: "error" });
    } finally {
      setDeleting(false);
    }
  }

  async function handleChangePassword(e: React.FormEvent) {
    e.preventDefault();
    setPwError("");
    if (pwForm.new_password !== pwForm.confirm_new_password) {
      setPwError(t("vault.passwordMismatch"));
      return;
    }
    setChangingPw(true);
    try {
      await authApi.changePassword({
        current_password: pwForm.current_password,
        new_password: pwForm.new_password,
      });
      setPwForm(EMPTY_PW_FORM);
      setShowChangePw(false);
      pushToast({ title: t("vault.passwordChanged"), tone: "success" });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setPwError(msg);
    } finally {
      setChangingPw(false);
    }
  }

  // ── Render ─────────────────────────────────────────────────────────────
  return (
    <div className="mx-auto max-w-4xl px-6 py-8">
      {/* Header */}
      <div className="mb-6 flex flex-wrap items-start justify-between gap-4">
        <div>
          <span className="rounded-lg bg-[#EFF6FF] px-2 py-0.5 text-[12px] font-semibold uppercase tracking-wider text-[#3A7ECC]">
            {t("vault.headerPill")}
          </span>
          <h1 className="mt-2 text-[24px] font-bold tracking-[-0.02em] text-[#0F172A]">
            {t("vault.title")}
          </h1>
          <p className="mt-1 text-[14px] text-[#64748B]">{t("vault.subtitle")}</p>
        </div>
        <div className="flex gap-2">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => { setShowChangePw((v) => !v); setPwError(""); }}
          >
            <KeyRound size={14} />
            {t("vault.changePassword")}
          </Button>
          <Button
            size="sm"
            onClick={() => { setShowCreate((v) => !v); setCreateError(""); }}
            disabled={sessionExpired}
          >
            <Plus size={14} />
            {t("vault.newSecret")}
          </Button>
        </div>
      </div>

      {/* Session-expired banner */}
      {sessionExpired && (
        <div className="mb-5">
          <InlineBanner
            tone="error"
            title={t("vault.sessionExpiredTitle")}
            description={t("vault.sessionExpiredDesc")}
          />
        </div>
      )}

      {/* Change password panel */}
      {showChangePw && (
        <Card className="mb-5" tone="raised">
          <CardHeader>
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2 text-[14px] font-semibold text-[#0F172A]">
                <KeyRound size={15} className="text-[#3A7ECC]" />
                {t("vault.changePassword")}
              </div>
              <button
                onClick={() => { setShowChangePw(false); setPwError(""); setPwForm(EMPTY_PW_FORM); }}
                className="text-[13px] text-[#94A3B8] hover:text-[#475569]"
              >
                {t("common.close")}
              </button>
            </div>
          </CardHeader>
          <CardContent>
            <form onSubmit={(e) => void handleChangePassword(e)} className="space-y-3">
              <div>
                <label className="mb-1 block text-[13px] font-medium text-[#374151]">
                  {t("vault.currentPassword")}
                </label>
                <Input
                  type="password"
                  required
                  value={pwForm.current_password}
                  onChange={(e) => setPwForm((f) => ({ ...f, current_password: e.target.value }))}
                />
              </div>
              <div>
                <label className="mb-1 block text-[13px] font-medium text-[#374151]">
                  {t("vault.newPassword")}
                </label>
                <Input
                  type="password"
                  required
                  minLength={8}
                  value={pwForm.new_password}
                  onChange={(e) => setPwForm((f) => ({ ...f, new_password: e.target.value }))}
                />
              </div>
              <div>
                <label className="mb-1 block text-[13px] font-medium text-[#374151]">
                  {t("vault.confirmNewPassword")}
                </label>
                <Input
                  type="password"
                  required
                  value={pwForm.confirm_new_password}
                  onChange={(e) => setPwForm((f) => ({ ...f, confirm_new_password: e.target.value }))}
                />
              </div>
              {pwError && <p className="text-[13px] text-red-600">{pwError}</p>}
              <div className="flex gap-2">
                <Button type="submit" size="sm" loading={changingPw}>
                  {t("common.save")}
                </Button>
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => { setShowChangePw(false); setPwError(""); setPwForm(EMPTY_PW_FORM); }}
                >
                  {t("common.cancel")}
                </Button>
              </div>
            </form>
          </CardContent>
        </Card>
      )}

      {/* Create form */}
      {showCreate && (
        <Card className="mb-5" tone="raised">
          <CardHeader>
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2 text-[14px] font-semibold text-[#0F172A]">
                <Lock size={15} className="text-[#3A7ECC]" />
                {t("vault.createTitle")}
              </div>
              <button
                onClick={() => { setShowCreate(false); setCreateError(""); setForm(EMPTY_FORM); }}
                className="text-[13px] text-[#94A3B8] hover:text-[#475569]"
              >
                {t("common.close")}
              </button>
            </div>
          </CardHeader>
          <CardContent>
            <form onSubmit={(e) => void handleCreate(e)} className="space-y-3">
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="mb-1 block text-[13px] font-medium text-[#374151]">
                    {t("vault.label")} *
                  </label>
                  <Input
                    required
                    placeholder="My API Key"
                    value={form.label}
                    onChange={(e) => setForm((f) => ({ ...f, label: e.target.value }))}
                  />
                </div>
                <div>
                  <label className="mb-1 block text-[13px] font-medium text-[#374151]">
                    {t("vault.secretType")}
                  </label>
                  <Input
                    placeholder={t("vault.typePlaceholder")}
                    value={form.secret_type}
                    onChange={(e) => setForm((f) => ({ ...f, secret_type: e.target.value }))}
                  />
                </div>
              </div>

              <div>
                <label className="mb-1 block text-[13px] font-medium text-[#374151]">
                  {t("vault.secretValue")} *
                </label>
                <Input
                  type="password"
                  required
                  placeholder="sk-…"
                  value={form.secret_value}
                  onChange={(e) => setForm((f) => ({ ...f, secret_value: e.target.value }))}
                />
                <p className="mt-1 text-[12px] text-[#94A3B8]">{t("vault.secretValueHint")}</p>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="mb-1 block text-[13px] font-medium text-[#374151]">
                    {t("vault.username")}
                  </label>
                  <Input
                    placeholder="username"
                    value={form.username}
                    onChange={(e) => setForm((f) => ({ ...f, username: e.target.value }))}
                  />
                </div>
                <div>
                  <label className="mb-1 block text-[13px] font-medium text-[#374151]">
                    {t("vault.url")}
                  </label>
                  <Input
                    placeholder="https://…"
                    value={form.url}
                    onChange={(e) => setForm((f) => ({ ...f, url: e.target.value }))}
                  />
                </div>
              </div>

              <div>
                <label className="mb-1 block text-[13px] font-medium text-[#374151]">
                  {t("vault.note")}
                </label>
                <Input
                  placeholder={t("common.optional")}
                  value={form.note}
                  onChange={(e) => setForm((f) => ({ ...f, note: e.target.value }))}
                />
              </div>

              <div>
                <label className="mb-1 flex items-center gap-1.5 text-[13px] font-medium text-[#374151]">
                  <Bot size={13} className="text-[#3A7ECC]" />
                  {t("vault.aiDescription")}
                </label>
                <textarea
                  rows={2}
                  placeholder={t("vault.aiDescriptionPlaceholder")}
                  value={form.ai_description}
                  onChange={(e) => setForm((f) => ({ ...f, ai_description: e.target.value }))}
                  className="w-full resize-none rounded-lg border border-[#E2E8F0] bg-white px-3 py-2 text-[13px] text-[#0F172A] placeholder-[#94A3B8] focus:border-[#3A7ECC] focus:outline-none focus:ring-1 focus:ring-[#3A7ECC]"
                />
                <p className="mt-1 text-[12px] text-[#94A3B8]">{t("vault.aiDescriptionHint")}</p>
              </div>

              {createError && <p className="text-[13px] text-red-600">{createError}</p>}

              <div className="flex gap-2">
                <Button type="submit" size="sm" loading={creating}>
                  {t("vault.newSecret")}
                </Button>
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => { setShowCreate(false); setForm(EMPTY_FORM); setCreateError(""); }}
                >
                  {t("common.cancel")}
                </Button>
              </div>
            </form>
          </CardContent>
        </Card>
      )}

      {/* Secrets list */}
      {loading ? (
        <div className="space-y-3">
          {[1, 2, 3].map((i) => <SkeletonBlock key={i} className="h-[4.5rem]" />)}
        </div>
      ) : secrets.length === 0 && !sessionExpired ? (
        <SectionEmpty
          title={t("vault.emptyTitle")}
          description={t("vault.emptyDesc")}
          action={
            <Button size="sm" onClick={() => setShowCreate(true)}>
              <Plus size={14} />
              {t("vault.newSecret")}
            </Button>
          }
        />
      ) : (
        <div className="space-y-3">
          {secrets.map((secret) => {
            const isRevealed = revealState?.id === secret.id;
            const isDeletePending = deleteTarget === secret.id;

            return (
              <Card key={secret.id} tone="default">
                <CardContent className="py-4">
                  <div className="flex items-start gap-4">
                    {/* Icon */}
                    <div className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-xl bg-[#EFF6FF]">
                      <Lock size={16} className="text-[#3A7ECC]" />
                    </div>

                    {/* Meta */}
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="text-[14px] font-semibold text-[#0F172A]">
                          {secret.label}
                        </span>
                        {secret.secret_type && (
                          <span className="rounded-full bg-[#F1F5F9] px-2 py-0.5 text-[11px] font-medium text-[#64748B]">
                            {secret.secret_type}
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 flex flex-wrap items-center gap-x-3 text-[12px] text-[#94A3B8]">
                        {secret.username && <span>{secret.username}</span>}
                        {secret.url && (
                          <span className="max-w-[18rem] truncate">{secret.url}</span>
                        )}
                        {secret.note && <span className="italic">{secret.note}</span>}
                        <span>{formatDate(secret.created_at)}</span>
                      </div>

                      {/* AI description — inline editable */}
                      {editAiTarget === secret.id ? (
                        <div className="mt-2 flex items-start gap-2">
                          <textarea
                            rows={2}
                            autoFocus
                            className="flex-1 resize-none rounded-lg border border-[#3A7ECC] bg-white px-2 py-1.5 text-[12px] text-[#0F172A] focus:outline-none focus:ring-1 focus:ring-[#3A7ECC]"
                            value={editAiValue}
                            onChange={(e) => setEditAiValue(e.target.value)}
                          />
                          <div className="flex flex-col gap-1">
                            <Button size="sm" loading={savingAi} onClick={() => void handleSaveAiDescription(secret.id)}>
                              {t("common.save")}
                            </Button>
                            <Button size="sm" variant="secondary" onClick={() => setEditAiTarget(null)}>
                              {t("common.cancel")}
                            </Button>
                          </div>
                        </div>
                      ) : (
                        <div className="mt-1.5 flex items-start gap-1.5">
                          <Bot size={12} className="mt-0.5 flex-shrink-0 text-[#3A7ECC]" />
                          <span
                            className={`flex-1 text-[12px] ${secret.ai_description ? "text-[#475569]" : "text-[#CBD5E1]"}`}
                          >
                            {secret.ai_description || t("vault.editAiHint")}
                          </span>
                          <button
                            onClick={() => { setEditAiTarget(secret.id); setEditAiValue(secret.ai_description ?? ""); }}
                            className="flex-shrink-0 text-[#CBD5E1] hover:text-[#3A7ECC]"
                            title={t("vault.editAiHint")}
                          >
                            <Pencil size={11} />
                          </button>
                        </div>
                      )}

                      {/* Revealed value — visible only for REVEAL_TTL_S seconds */}
                      {isRevealed && revealState && (
                        <div className="mt-2 rounded-xl border border-[#FDE68A] bg-[#FFFBEB] p-3">
                          <p className="mb-1 text-[11px] font-semibold text-[#B45309]">
                            {t("vault.copyWarning")} &mdash; {revealState.secondsLeft}s
                          </p>
                          <code className="break-all font-mono text-[13px] text-[#92400E]">
                            {revealState.value}
                          </code>
                        </div>
                      )}
                    </div>

                    {/* Actions */}
                    <div className="flex flex-shrink-0 items-center gap-2">
                      {/* Reveal / hide toggle */}
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => void handleReveal(secret.id)}
                        loading={revealing === secret.id}
                        title={isRevealed ? t("vault.hideValue") : t("vault.reveal")}
                      >
                        {isRevealed ? <EyeOff size={14} /> : <Eye size={14} />}
                        {isRevealed
                          ? `${revealState?.secondsLeft ?? 0}s`
                          : t("vault.reveal")}
                      </Button>

                      {/* Delete — two-step */}
                      {isDeletePending ? (
                        <div className="flex items-center gap-1.5">
                          <span className="text-[12px] text-[#94A3B8]">{t("common.confirm")}?</span>
                          <Button
                            variant="danger"
                            size="sm"
                            onClick={() => void handleDelete(secret.id)}
                            loading={deleting}
                          >
                            {t("common.delete")}
                          </Button>
                          <button
                            onClick={() => setDeleteTarget(null)}
                            className="text-[12px] text-[#94A3B8] hover:text-[#475569]"
                          >
                            {t("common.cancel")}
                          </button>
                        </div>
                      ) : (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => setDeleteTarget(secret.id)}
                          className="text-[#CBD5E1] hover:text-red-400"
                          title={t("common.delete")}
                        >
                          <Trash2 size={14} />
                        </Button>
                      )}
                    </div>
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}
