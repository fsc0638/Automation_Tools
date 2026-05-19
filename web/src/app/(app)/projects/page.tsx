"use client";
import { useCallback, useEffect, useMemo, useState, type FormEvent, type MouseEvent, type ReactNode } from "react";
import { useRouter } from "next/navigation";
import {
  Archive,
  ArchiveRestore,
  Clock,
  FolderOpen,
  GitBranch,
  KeyRound,
  Plus,
  Search,
  Sparkles,
  Trash2,
  Upload,
} from "lucide-react";
import { gitIdentities, projects as projectsApi, type GitIdentity, type Project, type WorkspaceKind } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, InlineBanner, SectionEmpty, SkeletonBlock } from "@/components/ui/card";
import { formatDate } from "@/lib/utils";
import { useToastStore } from "@/lib/toast-store";
import { useT } from "@/lib/i18n";

const emptyProjectForm = {
  name: "",
  description: "",
  source_type: "local",
  source_path: "",
  git_identity_id: "",
  default_branch: "main",
  kind: "code" as WorkspaceKind,
};

const emptyIdentityForm = {
  name: "",
  provider: "github",
  username: "",
  access_token: "",
};

export default function ProjectsPage() {
  const router = useRouter();
  const pushToast = useToastStore((state) => state.pushToast);
  const t = useT();
  const [projectList, setProjectList] = useState<Project[]>([]);
  const [identityList, setIdentityList] = useState<GitIdentity[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);
  const [showIdentityCreate, setShowIdentityCreate] = useState(false);
  const [form, setForm] = useState(emptyProjectForm);
  const [identityForm, setIdentityForm] = useState(emptyIdentityForm);
  const [creating, setCreating] = useState(false);
  const [identityCreating, setIdentityCreating] = useState(false);
  const [error, setError] = useState("");
  const [identityError, setIdentityError] = useState("");
  const [remoteBranches, setRemoteBranches] = useState<string[]>([]);
  const [loadingBranches, setLoadingBranches] = useState(false);
  const [branchFetchError, setBranchFetchError] = useState("");
  const [search, setSearch] = useState("");
  const [sourceFilter, setSourceFilter] = useState<"all" | "git" | "local">("all");
  const [uploadFile, setUploadFile] = useState<File | null>(null);

  useEffect(() => { void load(); }, []);

  async function load() {
    try {
      const [projects, identities] = await Promise.all([
        projectsApi.list(),
        gitIdentities.list().catch(() => []),
      ]);
      setProjectList(projects);
      setIdentityList(identities);
    } finally {
      setLoading(false);
    }
  }

  const fetchRemoteBranches = useCallback(async () => {
    if (form.source_type !== "git" || !form.source_path.trim()) return;
    setLoadingBranches(true);
    setBranchFetchError("");
    try {
      const result = await projectsApi.remoteBranches(
        form.source_path.trim(),
        form.git_identity_id || undefined,
      );
      setRemoteBranches(result.branches);
      if (result.branches.length === 0) {
        setBranchFetchError("No branches found. The repo may be private or the URL/token may be incorrect.");
      } else {
        setForm((current) => {
          if (result.branches.includes(current.default_branch)) return current;
          const best = result.branches.includes("main") ? "main"
            : result.branches.includes("master") ? "master"
            : result.branches[0];
          return { ...current, default_branch: best };
        });
      }
    } catch (err) {
      setRemoteBranches([]);
      setBranchFetchError(err instanceof Error ? err.message : "Failed to fetch branches");
    } finally {
      setLoadingBranches(false);
    }
  }, [form.git_identity_id, form.source_path, form.source_type]);

  useEffect(() => {
    if (form.source_type !== "git" || !form.source_path.trim()) return;
    const timer = window.setTimeout(() => {
      void fetchRemoteBranches();
    }, 600);
    return () => window.clearTimeout(timer);
  }, [fetchRemoteBranches, form.git_identity_id, form.source_path, form.source_type]);

  async function handleCreate(e: FormEvent) {
    e.preventDefault();
    setError("");
    setCreating(true);
    try {
      if (form.kind !== "code") {
        // 行政庶務工作區：無 repo，只送名稱/描述/kind，後端跳過 clone/index。
        await projectsApi.create({
          name: form.name,
          description: form.description || undefined,
          source_type: "local",
          source_path: "",
          kind: form.kind,
        });
      } else if (form.source_type === "upload") {
        if (!uploadFile) throw new Error("Please choose a zip file to upload");
        await projectsApi.upload({
          name: form.name,
          description: form.description || undefined,
          file: uploadFile,
        });
      } else {
        await projectsApi.create({
          name: form.name,
          description: form.description || undefined,
          source_type: form.source_type,
          source_path: form.source_path,
          git_identity_id: form.source_type === "git" && form.git_identity_id ? form.git_identity_id : undefined,
          default_branch: form.source_type === "git" ? form.default_branch || "main" : undefined,
          kind: "code",
        });
      }
      setShowCreate(false);
      setForm(emptyProjectForm);
      setUploadFile(null);
      setRemoteBranches([]);
      await load();
      pushToast({ tone: "success", title: "Project created", description: "The new workspace is ready in your dashboard." });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to create project";
      setError(message);
      pushToast({ tone: "error", title: "Project creation failed", description: message });
    } finally {
      setCreating(false);
    }
  }

  async function handleCreateIdentity(e: FormEvent) {
    e.preventDefault();
    setIdentityError("");
    setIdentityCreating(true);
    try {
      const identity = await gitIdentities.create({
        ...identityForm,
        repository_url: showCreate && form.source_type === "git" && form.source_path.trim()
          ? form.source_path.trim()
          : undefined,
      });
      setIdentityList((items) => [identity, ...items]);
      setIdentityForm(emptyIdentityForm);
      setShowIdentityCreate(false);
      setForm((current) => ({ ...current, git_identity_id: identity.id }));
      pushToast({ tone: "success", title: "Git profile saved", description: `${identity.name} can now be reused across projects.` });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to create Git identity";
      setIdentityError(message);
      pushToast({ tone: "error", title: "Git profile creation failed", description: message });
    } finally {
      setIdentityCreating(false);
    }
  }

  async function handleDelete(id: string, e: MouseEvent) {
    e.stopPropagation();
    if (!confirm(t("projects.confirmDelete"))) return;
    await projectsApi.delete(id);
    await load();
    pushToast({ tone: "warning", title: "Project deleted", description: "The workspace has been removed from your dashboard." });
  }

  async function handleArchive(id: string, archived: boolean, e: MouseEvent) {
    e.stopPropagation();
    await projectsApi.archive(id, archived);
    await load();
    pushToast({
      tone: archived ? "warning" : "success",
      title: archived ? "已封存" : "已取消封存",
      description: archived ? "工作區已淡化，資料與待辦皆保留。" : "工作區已恢復為使用中。",
    });
  }

  async function handleDeleteIdentity(id: string) {
    if (!confirm(t("identity.confirmDelete"))) return;
    await gitIdentities.delete(id);
    await load();
    pushToast({ tone: "warning", title: "Git profile deleted", description: "Projects may need another profile for future repository access." });
  }

  const filteredProjects = useMemo(() => {
    const query = search.trim().toLowerCase();
    return projectList.filter((project) => {
      const sourceMatch = sourceFilter === "all" || project.source_type === sourceFilter;
      const textMatch = !query || `${project.name} ${project.description ?? ""} ${project.default_branch ?? ""} ${project.source_path}`
        .toLowerCase()
        .includes(query);
      return sourceMatch && textMatch;
    });
  }, [projectList, search, sourceFilter]);

  const gitProjects = projectList.filter((project) => project.source_type === "git").length;
  const localProjects = projectList.filter((project) => project.source_type === "local").length;
  const recentProjects = [...projectList]
    .sort((a, b) => +new Date(b.updated_at) - +new Date(a.updated_at))
    .slice(0, 3);
  const latestUpdate = recentProjects[0]?.updated_at;

  return (
    <div className="mx-auto flex max-w-7xl flex-col gap-6 p-8">
      <section className="rounded-[28px] border border-[#E2E8F0] bg-white p-6 shadow-sm">
        <div className="flex flex-col gap-5 xl:flex-row xl:items-center xl:justify-between">
          <div>
            <div className="inline-flex items-center gap-2 rounded-full bg-[#EEF4FF] px-3 py-1 text-[13px] font-semibold tracking-[-0.01em] text-[#0050A0]">
              <Sparkles size={13} /> AI workspace dashboard
            </div>
            <h1 className="type-page-title mt-3 max-w-3xl">{t("projects.title")}</h1>
            <p className="type-body-muted mt-3 max-w-2xl">
              {t("projects.subtitle")}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button variant="secondary" onClick={() => setShowIdentityCreate((value) => !value)}>
              <KeyRound size={16} /> {t("projects.gitIdentity")}
            </Button>
            <Button onClick={() => setShowCreate(true)}>
              <Plus size={16} /> {t("projects.newProject")}
            </Button>
          </div>
        </div>

        <div className="mt-6 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          <SummaryCard label="Total workspaces" value={String(projectList.length)} helper="All AI-enabled projects" />
          <SummaryCard label="Git connected" value={String(gitProjects)} helper="Repositories with branch context" tone="blue" />
          <SummaryCard label="Local folders" value={String(localProjects)} helper="On-disk workspaces without remote sync" />
          <SummaryCard label="Git profiles" value={String(identityList.length)} helper={latestUpdate ? `Latest activity ${formatDate(latestUpdate)}` : "No activity yet"} tone="violet" />
        </div>
      </section>

      <section className="flex flex-col gap-3 rounded-[24px] border border-[#E2E8F0] bg-white p-5 shadow-sm xl:flex-row xl:items-center xl:justify-between">
        <div className="flex flex-1 items-center gap-3 rounded-2xl border border-[#E2E8F0] bg-[#F8FAFC] px-4 py-3">
          <Search size={16} className="text-[#94A3B8]" />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("projects.searchPlaceholder")}
            className="w-full bg-transparent text-[15px] text-[#1A1A2E] outline-none placeholder:text-[#94A3B8]"
          />
        </div>
        <div className="flex flex-wrap gap-2">
          {([
            ["all", `${t("projects.filterAll")} (${projectList.length})`],
            ["git", `${t("projects.filterGit")} (${gitProjects})`],
            ["local", `${t("projects.filterLocal")} (${localProjects})`],
          ] as const).map(([value, label]) => (
            <button
              key={value}
              onClick={() => setSourceFilter(value)}
              className={sourceFilter === value
                ? "rounded-xl border border-[#BFDBFE] bg-[#EFF6FF] px-3 py-2 text-sm font-medium text-[#0050A0]"
                : "rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-sm text-[#64748B] hover:border-[#94A3B8] hover:text-[#1A1A2E]"}
            >
              {label}
            </button>
          ))}
        </div>
      </section>

      {showIdentityCreate && (
        <Card className="rounded-[24px] p-6 shadow-sm">
          <div className="mb-4 flex items-start justify-between gap-4">
            <div>
              <h2 className="type-section-title text-[1.35rem]">{t("projects.addGitIdentity")}</h2>
              <p className="type-body-muted mt-2">{t("identity.tokenHint")}</p>
            </div>
          </div>
          <form onSubmit={handleCreateIdentity} className="flex flex-col gap-4">
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
              <Input id="git-name" label={t("identity.displayName")} placeholder="Work GitHub" value={identityForm.name}
                onChange={(e) => setIdentityForm((current) => ({ ...current, name: e.target.value }))} required />
              <Input id="git-provider" label={t("identity.provider")} placeholder="github / gitlab / generic" value={identityForm.provider}
                onChange={(e) => setIdentityForm((current) => ({ ...current, provider: e.target.value }))} />
              <Input id="git-user" label={t("identity.gitUsername")} placeholder="username" value={identityForm.username}
                onChange={(e) => setIdentityForm((current) => ({ ...current, username: e.target.value }))} required />
              <Input id="git-token" label={t("identity.accessToken")} type="password" placeholder="Personal access token" value={identityForm.access_token}
                onChange={(e) => setIdentityForm((current) => ({ ...current, access_token: e.target.value }))} required />
            </div>
            {identityError && (
              <InlineBanner
                tone="error"
                title="Git profile could not be saved"
                description={identityError}
              />
            )}
            <div className="flex gap-3 pt-2">
              <Button type="submit" loading={identityCreating}>{t("identity.saveIdentity")}</Button>
              <Button type="button" variant="secondary" onClick={() => setShowIdentityCreate(false)}>{t("common.cancel")}</Button>
            </div>
          </form>

          {identityList.length > 0 && (
            <div className="mt-6 grid gap-3 md:grid-cols-2">
              {identityList.map((identity) => (
                <div key={identity.id} className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE] p-4">
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <div className="text-sm font-semibold text-[#1A1A2E]">{identity.name}</div>
                      <div className="mt-1 text-xs text-[#64748B]">{identity.provider} · {identity.username}</div>
                    </div>
                    <button type="button" onClick={() => void handleDeleteIdentity(identity.id)} className="text-[#94A3B8] hover:text-[#C8102E]">
                      <Trash2 size={14} />
                    </button>
                  </div>
                  <div className="mt-3 text-xs text-[#94A3B8]">Created {formatDate(identity.created_at)}</div>
                </div>
              ))}
            </div>
          )}
        </Card>
      )}

      {showCreate && (
        <Card className="rounded-[24px] p-6 shadow-sm">
          <div className="mb-4">
            <h2 className="type-section-title text-[1.35rem]">Create Project Workspace</h2>
            <p className="type-body-muted mt-2">Connect a local folder or a Git repository and make it available to the AI workspace.</p>
          </div>
          <form onSubmit={handleCreate} className="flex flex-col gap-5">
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
              <Input id="pname" label={t("projects.projectName")} placeholder="My Project" value={form.name}
                onChange={(e) => setForm((current) => ({ ...current, name: e.target.value }))} required />
              <Input id="desc" label={`${t("projects.description")} ${t("common.optional")}`} placeholder={t("projects.descriptionHint")} value={form.description}
                onChange={(e) => setForm((current) => ({ ...current, description: e.target.value }))} />
            </div>

            <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
              {([
                { v: "code", label: "程式碼工作區", hint: "Git 倉庫 / 本機資料夾，含索引與 AI 接地" },
                { v: "admin", label: "行政工作區", hint: "行政庶務 / 個人事務；無倉庫，只管待辦與會議" },
              ] as { v: WorkspaceKind; label: string; hint: string }[]).map(({ v, label, hint }) => (
                <button key={v} type="button"
                  onClick={() => setForm((current) => ({ ...current, kind: v }))}
                  className={form.kind === v
                    ? "rounded-2xl border border-[#BFDBFE] bg-[#EFF6FF] p-4 text-left"
                    : "rounded-2xl border border-[#E2E8F0] bg-white p-4 text-left hover:border-[#94A3B8]"}>
                  <div className="text-sm font-semibold text-[#1A1A2E]">{label}</div>
                  <div className="mt-1 text-xs text-[#64748B]">{hint}</div>
                </button>
              ))}
            </div>

            {form.kind === "code" && (<>
            <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
              {[
                { value: "local", label: t("projects.sourceLocal"), hint: t("projects.sourceLocalHint"), icon: FolderOpen },
                { value: "git", label: t("projects.sourceGit"), hint: t("projects.sourceGitHint"), icon: GitBranch },
                { value: "upload", label: t("projects.sourceUpload"), hint: t("projects.sourceUploadHint"), icon: Upload },
              ].map(({ value, label, hint, icon: Icon }) => (
                <button
                  key={value}
                  type="button"
                  onClick={() => setForm((current) => ({ ...current, source_type: value }))}
                  className={form.source_type === value
                    ? "rounded-2xl border border-[#BFDBFE] bg-[#EFF6FF] p-4 text-left"
                    : "rounded-2xl border border-[#E2E8F0] bg-white p-4 text-left hover:border-[#94A3B8]"}
                >
                  <div className="flex items-start gap-3">
                    <div className="rounded-xl bg-white p-2 shadow-sm"><Icon size={16} className="text-[#0050A0]" /></div>
                    <div>
                      <div className="text-sm font-semibold text-[#1A1A2E]">{label}</div>
                      <div className="mt-1 text-xs text-[#64748B]">{hint}</div>
                    </div>
                  </div>
                </button>
              ))}
            </div>

            {form.source_type !== "upload" && (
              <Input id="path" label={form.source_type === "local" ? t("projects.folderPath") : t("projects.gitUrl")}
                placeholder={form.source_type === "local" ? "C:/Projects/my-app" : "https://github.com/org/repo.git"}
                value={form.source_path}
                onChange={(e) => {
                  setForm((current) => ({ ...current, source_path: e.target.value }));
                  setRemoteBranches([]);
                  setBranchFetchError("");
                }} required />
            )}

            {form.source_type === "upload" && (
              <div className="flex flex-col gap-1.5">
                <label className="text-sm font-medium text-[#1A1A2E]">{t("projects.uploadZip")}</label>
                <input
                  type="file"
                  accept=".zip,application/zip,application/x-zip-compressed"
                  onChange={(e) => setUploadFile(e.target.files?.[0] ?? null)}
                  className="block w-full rounded-xl border border-dashed border-[#94A3B8] bg-[#F8FAFC] px-3 py-3 text-sm text-[#64748B] file:mr-3 file:rounded-md file:border-0 file:bg-[#0050A0] file:px-3 file:py-1.5 file:text-xs file:font-medium file:text-white hover:border-[#0050A0]"
                  required
                />
                <p className="text-xs text-[#94A3B8]">{t("projects.uploadHint")}</p>
              </div>
            )}

            {form.source_type === "git" && (
              <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                <div className="flex flex-col gap-1.5">
                  <label className="text-sm font-medium text-[#1A1A2E]">{t("projects.gitProfile")}</label>
                  <select value={form.git_identity_id}
                    onChange={(e) => {
                      setForm((current) => ({ ...current, git_identity_id: e.target.value }));
                      setRemoteBranches([]);
                      setBranchFetchError("");
                    }}
                    className="h-11 rounded-xl border border-[#E2E8F0] px-3 text-sm text-[#1A1A2E] bg-white">
                    <option value="">{t("projects.noProfile")}</option>
                    {identityList.map((identity) => (
                      <option key={identity.id} value={identity.id}>{identity.name} · {identity.username}</option>
                    ))}
                  </select>
                </div>
                <div className="flex flex-col gap-1.5">
                  <label className="text-sm font-medium text-[#1A1A2E]">{t("projects.branch")}</label>
                  <div className="flex gap-2">
                    {remoteBranches.length > 0 ? (
                      <select
                        value={form.default_branch}
                        onChange={(e) => setForm((current) => ({ ...current, default_branch: e.target.value }))}
                        className="h-11 flex-1 rounded-xl border border-[#E2E8F0] px-3 text-sm text-[#1A1A2E] bg-white focus:outline-none focus:ring-2 focus:ring-[#0050A0]"
                      >
                        {remoteBranches.map((branch) => (
                          <option key={branch} value={branch}>{branch}</option>
                        ))}
                      </select>
                    ) : (
                      <input
                        value={form.default_branch}
                        onChange={(e) => setForm((current) => ({ ...current, default_branch: e.target.value }))}
                        placeholder="main"
                        className="h-11 flex-1 rounded-xl border border-[#E2E8F0] px-3 text-sm text-[#1A1A2E] bg-white focus:outline-none focus:ring-2 focus:ring-[#0050A0]"
                      />
                    )}
                    <button
                      type="button"
                      onClick={() => void fetchRemoteBranches()}
                      disabled={loadingBranches || !form.source_path}
                      title={t("projects.fetchBranches")}
                      className="flex h-11 items-center justify-center rounded-xl border border-[#E2E8F0] px-3 text-[#64748B] hover:border-[#0050A0] hover:text-[#0050A0] disabled:opacity-40"
                    >
                      {loadingBranches ? (
                        <span className="inline-block h-3.5 w-3.5 animate-spin rounded-full border-2 border-current border-t-transparent" />
                      ) : (
                        <GitBranch size={14} />
                      )}
                    </button>
                  </div>
                  {branchFetchError && (
                    <InlineBanner
                      tone="warning"
                      title="Could not fetch remote branches"
                      description={branchFetchError}
                    />
                  )}
                </div>
              </div>
            )}
            </>)}

            {error && (
              <InlineBanner
                tone="error"
                title="Project setup needs attention"
                description={error}
              />
            )}

            <div className="flex gap-3 pt-2">
              <Button type="submit" loading={creating}>{t("projects.createProject")}</Button>
              <Button type="button" variant="secondary" onClick={() => setShowCreate(false)}>{t("common.cancel")}</Button>
            </div>
          </form>
        </Card>
      )}

      {loading ? (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
          {Array.from({ length: 4 }).map((_, index) => (
            <Card key={index} className="rounded-[24px] p-5">
              <div className="flex items-start gap-3">
                <SkeletonBlock className="h-11 w-11 flex-shrink-0" />
                <div className="min-w-0 flex-1 space-y-3">
                  <SkeletonBlock className="h-5 w-40" />
                  <SkeletonBlock className="h-4 w-full" />
                  <SkeletonBlock className="h-4 w-3/4" />
                </div>
              </div>
              <div className="mt-5 grid gap-3 md:grid-cols-2">
                <SkeletonBlock className="h-16 w-full" />
                <SkeletonBlock className="h-16 w-full" />
                <SkeletonBlock className="h-16 w-full" />
                <SkeletonBlock className="h-16 w-full" />
              </div>
            </Card>
          ))}
        </div>
      ) : filteredProjects.length === 0 ? (
        <SectionEmpty
          className="bg-white px-6 py-20 shadow-sm rounded-[24px]"
          title={search || sourceFilter !== "all" ? "No project matches your current view" : "No workspaces yet"}
          description={search || sourceFilter !== "all"
            ? "Try another search term, switch filters, or create a new workspace."
            : "Create your first local folder or connect a Git repository to start a project-aware workspace."}
          action={
            <div className="flex flex-wrap justify-center gap-3">
              <Button onClick={() => setShowCreate(true)}><Plus size={14} /> New Project</Button>
              {(search || sourceFilter !== "all") && (
                <Button variant="secondary" onClick={() => { setSearch(""); setSourceFilter("all"); }}>
                  Clear Filters
                </Button>
              )}
            </div>
          }
        />
      ) : (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
          {filteredProjects.map((project) => {
            const linkedIdentity = project.git_identity_id
              ? identityList.find((identity) => identity.id === project.git_identity_id)
              : null;
            return (
              <Card
                key={project.id}
                className={`group rounded-[24px] border border-[#E2E8F0] p-5 shadow-sm transition hover:-translate-y-0.5 hover:border-[#BFDBFE] hover:shadow-md${project.archived_at ? " opacity-60" : ""}`}
                onClick={() => router.push(`/projects/${project.id}`)}
              >
                <div className="flex items-start justify-between gap-4">
                  <div className="flex min-w-0 items-start gap-3">
                    <div className="flex h-11 w-11 flex-shrink-0 items-center justify-center rounded-2xl bg-[#F1F5F9] text-[#0050A0]">
                      {project.source_type === "git" ? <GitBranch size={18} /> : <FolderOpen size={18} />}
                    </div>
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <h3 className="truncate text-base font-semibold text-[#1A1A2E] group-hover:text-[#0050A0]">{project.name}</h3>
                        <Badge>{project.kind && project.kind !== "code" ? "行政" : project.source_type}</Badge>
                        {project.source_type === "git" && project.default_branch && <Badge tone="blue">{project.default_branch}</Badge>}
                        {project.archived_at && <Badge>已封存</Badge>}
                      </div>
                      <p className="mt-2 line-clamp-2 text-sm text-[#64748B]">{project.description || project.source_path}</p>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      title={project.archived_at ? "取消封存" : "封存（淡化，不刪除）"}
                      onClick={(e) => void handleArchive(project.id, !project.archived_at, e)}
                      className="opacity-0 text-[#94A3B8] transition group-hover:opacity-100 hover:text-[#0050A0]"
                    >
                      {project.archived_at ? <ArchiveRestore size={15} /> : <Archive size={15} />}
                    </button>
                    <button
                      onClick={(e) => void handleDelete(project.id, e)}
                      className="opacity-0 text-[#94A3B8] transition group-hover:opacity-100 hover:text-[#C8102E]"
                    >
                      <Trash2 size={15} />
                    </button>
                  </div>
                </div>

                <div className="mt-5 grid gap-3 md:grid-cols-2">
                  <InfoTile icon={<Clock size={12} />} label="Updated" value={formatDate(project.updated_at)} />
                  <InfoTile icon={<GitBranch size={12} />} label="Branch" value={project.default_branch ?? "—"} />
                  <InfoTile icon={<FolderOpen size={12} />} label="Path" value={project.source_path} truncate />
                  <InfoTile icon={<KeyRound size={12} />} label="Git Profile" value={linkedIdentity ? `${linkedIdentity.name} · ${linkedIdentity.username}` : "Not linked"} truncate />
                </div>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}

function SummaryCard({ label, value, helper, tone = "default" }: { label: string; value: string; helper: string; tone?: "default" | "blue" | "violet" }) {
  const toneClass = tone === "blue"
    ? "bg-[#EFF6FF] border-[#BFDBFE]"
    : tone === "violet"
      ? "bg-[#F5F3FF] border-[#DDD6FE]"
      : "bg-white border-[#E2E8F0]";
  return (
    <div className={`rounded-2xl border px-4 py-4 ${toneClass}`}>
      <div className="text-sm text-[#64748B]">{label}</div>
      <div className="mt-2 text-2xl font-semibold text-[#1A1A2E]">{value}</div>
      <div className="mt-1 text-xs text-[#94A3B8]">{helper}</div>
    </div>
  );
}

function Badge({ children, tone = "default" }: { children: ReactNode; tone?: "default" | "blue" }) {
  return (
    <span className={tone === "blue"
      ? "rounded-full bg-[#EFF6FF] px-2 py-0.5 text-xs font-medium text-[#0050A0]"
      : "rounded-full bg-[#F1F5F9] px-2 py-0.5 text-xs font-medium capitalize text-[#64748B]"}>
      {children}
    </span>
  );
}

function InfoTile({ icon, label, value, truncate = false }: { icon: ReactNode; label: string; value: string; truncate?: boolean }) {
  return (
    <div className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE] px-3 py-3">
      <div className="flex items-center gap-1.5 text-xs text-[#94A3B8]">
        {icon}
        {label}
      </div>
      <div className={`mt-1 text-sm font-medium text-[#334155] ${truncate ? "truncate" : ""}`}>{value}</div>
    </div>
  );
}
