"use client";
import { useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Plus, GitBranch, FolderOpen, Trash2, Clock, KeyRound } from "lucide-react";
import { gitIdentities, projects as projectsApi, type GitIdentity, type Project } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card } from "@/components/ui/card";
import { formatDate } from "@/lib/utils";

const emptyProjectForm = {
  name: "",
  description: "",
  source_type: "local",
  source_path: "",
  git_identity_id: "",
  default_branch: "main",
};

const emptyIdentityForm = {
  name: "",
  provider: "github",
  username: "",
  access_token: "",
};

export default function ProjectsPage() {
  const router = useRouter();
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
        setForm((f) => {
          if (result.branches.includes(f.default_branch)) return f;
          const best = result.branches.includes("main") ? "main"
            : result.branches.includes("master") ? "master"
            : result.branches[0];
          return { ...f, default_branch: best };
        });
      }
    } catch (err) {
      setRemoteBranches([]);
      setBranchFetchError(err instanceof Error ? err.message : "Failed to fetch branches");
    } finally {
      setLoadingBranches(false);
    }
  }, [form.source_type, form.source_path, form.git_identity_id]);

  useEffect(() => {
    if (form.source_type !== "git" || !form.source_path.trim()) return;
    const timer = window.setTimeout(() => {
      void fetchRemoteBranches();
    }, 600);
    return () => window.clearTimeout(timer);
  }, [form.source_type, form.source_path, form.git_identity_id, fetchRemoteBranches]);

  async function handleCreate(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setCreating(true);
    try {
      await projectsApi.create({
        name: form.name,
        description: form.description || undefined,
        source_type: form.source_type,
        source_path: form.source_path,
        git_identity_id: form.source_type === "git" && form.git_identity_id ? form.git_identity_id : undefined,
        default_branch: form.source_type === "git" ? form.default_branch || "main" : undefined,
      });
      setShowCreate(false);
      setForm(emptyProjectForm);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create project");
    } finally {
      setCreating(false);
    }
  }

  async function handleCreateIdentity(e: React.FormEvent) {
    e.preventDefault();
    setIdentityError("");
    setIdentityCreating(true);
    try {
      const identity = await gitIdentities.create({
        ...identityForm,
        repository_url: form.source_type === "git" && form.source_path.trim()
          ? form.source_path.trim()
          : undefined,
      });
      setIdentityList((items) => [identity, ...items]);
      setIdentityForm(emptyIdentityForm);
      setShowIdentityCreate(false);
      setForm((f) => ({ ...f, git_identity_id: identity.id }));
    } catch (err) {
      setIdentityError(err instanceof Error ? err.message : "Failed to create Git identity");
    } finally {
      setIdentityCreating(false);
    }
  }

  async function handleDelete(id: string, e: React.MouseEvent) {
    e.stopPropagation();
    if (!confirm("Delete this project?")) return;
    await projectsApi.delete(id);
    await load();
  }

  async function handleDeleteIdentity(id: string) {
    if (!confirm("Delete this Git identity? Existing cloned projects will remain, but future fetch/checkout may need credentials.")) return;
    await gitIdentities.delete(id);
    await load();
  }

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold text-[#1A1A2E]">Projects</h1>
          <p className="text-[#64748B] text-sm mt-1">Manage local folders, authorized Git users, repositories, and branches</p>
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => setShowIdentityCreate((v) => !v)}>
            <KeyRound size={16} /> Git Identity
          </Button>
          <Button onClick={() => setShowCreate(true)}>
            <Plus size={16} /> New Project
          </Button>
        </div>
      </div>

      {showIdentityCreate && (
        <Card className="mb-6 p-6">
          <h2 className="font-semibold text-[#1A1A2E] mb-4">Add Git Identity</h2>
          <form onSubmit={handleCreateIdentity} className="flex flex-col gap-4">
            <div className="grid grid-cols-2 gap-4">
              <Input id="git-name" label="Display Name" placeholder="Work GitHub" value={identityForm.name}
                onChange={(e) => setIdentityForm((f) => ({ ...f, name: e.target.value }))} required />
              <Input id="git-provider" label="Provider" placeholder="github / gitlab / generic" value={identityForm.provider}
                onChange={(e) => setIdentityForm((f) => ({ ...f, provider: e.target.value }))} />
              <Input id="git-user" label="Git Username" placeholder="username" value={identityForm.username}
                onChange={(e) => setIdentityForm((f) => ({ ...f, username: e.target.value }))} required />
              <Input id="git-token" label="Access Token" type="password" placeholder="Personal access token" value={identityForm.access_token}
                onChange={(e) => setIdentityForm((f) => ({ ...f, access_token: e.target.value }))} required />
            </div>
            <p className="text-xs text-[#94A3B8]">Token is stored server-side and hidden from API responses. Use a least-privilege token for repo read access.</p>
            {identityError && <p className="text-sm text-[#C8102E]">{identityError}</p>}
            <div className="flex gap-3 pt-2">
              <Button type="submit" loading={identityCreating}>Save Git Identity</Button>
              <Button type="button" variant="secondary" onClick={() => setShowIdentityCreate(false)}>Cancel</Button>
            </div>
          </form>

          {identityList.length > 0 && (
            <div className="mt-5 border-t border-[#E2E8F0] pt-4 space-y-2">
              {identityList.map((identity) => (
                <div key={identity.id} className="flex items-center justify-between text-sm bg-[#F8FAFC] rounded-lg px-3 py-2">
                  <span className="text-[#1A1A2E] font-medium">{identity.name}</span>
                  <span className="text-[#64748B]">{identity.provider} · {identity.username}</span>
                  <button type="button" onClick={() => handleDeleteIdentity(identity.id)} className="text-[#94A3B8] hover:text-[#C8102E]">
                    <Trash2 size={13} />
                  </button>
                </div>
              ))}
            </div>
          )}
        </Card>
      )}

      {showCreate && (
        <Card className="mb-6 p-6">
          <h2 className="font-semibold text-[#1A1A2E] mb-4">Create Project</h2>
          <form onSubmit={handleCreate} className="flex flex-col gap-4">
            <div className="grid grid-cols-2 gap-4">
              <Input id="pname" label="Project Name" placeholder="My Project" value={form.name}
                onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} required />
              <Input id="desc" label="Description (optional)" placeholder="Brief description" value={form.description}
                onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))} />
            </div>

            <div className="flex flex-col gap-1.5">
              <label className="text-sm font-medium text-[#1A1A2E]">Source Type</label>
              <div className="flex gap-3">
                {["local", "git"].map((t) => (
                  <button key={t} type="button"
                    onClick={() => setForm((f) => ({ ...f, source_type: t }))}
                    className={`flex items-center gap-2 px-4 py-2 rounded-lg border text-sm transition-colors ${
                      form.source_type === t
                        ? "border-[#0050A0] bg-blue-50 text-[#0050A0]"
                        : "border-[#E2E8F0] text-[#64748B] hover:border-[#94A3B8]"
                    }`}>
                    {t === "local" ? <FolderOpen size={14} /> : <GitBranch size={14} />}
                    {t === "local" ? "Local Folder" : "Git Repository"}
                  </button>
                ))}
              </div>
            </div>

            <Input id="path" label={form.source_type === "local" ? "Folder Path" : "Git URL"}
              placeholder={form.source_type === "local" ? "C:/Projects/my-app" : "https://github.com/org/repo.git"}
              value={form.source_path}
              onChange={(e) => {
                setForm((f) => ({ ...f, source_path: e.target.value }));
                setRemoteBranches([]);
                setBranchFetchError("");
              }} required />

            {form.source_type === "git" && (
              <div className="grid grid-cols-2 gap-4">
                <div className="flex flex-col gap-1.5">
                  <label className="text-sm font-medium text-[#1A1A2E]">Authorized Git User</label>
                  <select value={form.git_identity_id}
                    onChange={(e) => {
                      setForm((f) => ({ ...f, git_identity_id: e.target.value }));
                      setRemoteBranches([]);
                      setBranchFetchError("");
                    }}
                    className="h-10 rounded-lg border border-[#E2E8F0] px-3 text-sm text-[#1A1A2E] bg-white">
                    <option value="">No identity / public repo</option>
                    {identityList.map((identity) => (
                      <option key={identity.id} value={identity.id}>{identity.name} · {identity.username}</option>
                    ))}
                  </select>
                </div>
                <div className="flex flex-col gap-1.5">
                  <label className="text-sm font-medium text-[#1A1A2E]">Branch</label>
                  <div className="flex gap-2">
                    {remoteBranches.length > 0 ? (
                      <select
                        value={form.default_branch}
                        onChange={(e) => setForm((f) => ({ ...f, default_branch: e.target.value }))}
                        className="flex-1 h-10 rounded-lg border border-[#E2E8F0] px-3 text-sm text-[#1A1A2E] bg-white focus:outline-none focus:ring-2 focus:ring-[#0050A0]">
                        {remoteBranches.map((b) => (
                          <option key={b} value={b}>{b}</option>
                        ))}
                      </select>
                    ) : (
                      <input
                        value={form.default_branch}
                        onChange={(e) => setForm((f) => ({ ...f, default_branch: e.target.value }))}
                        placeholder="main"
                        className="flex-1 h-10 rounded-lg border border-[#E2E8F0] px-3 text-sm text-[#1A1A2E] bg-white focus:outline-none focus:ring-2 focus:ring-[#0050A0]"
                      />
                    )}
                    <button
                      type="button"
                      onClick={fetchRemoteBranches}
                      disabled={loadingBranches || !form.source_path}
                      title="Fetch branches from remote"
                      className="px-3 h-10 rounded-lg border border-[#E2E8F0] text-[#64748B] hover:border-[#0050A0] hover:text-[#0050A0] disabled:opacity-40 flex items-center justify-center">
                      {loadingBranches ? (
                        <span className="w-3.5 h-3.5 border-2 border-current border-t-transparent rounded-full animate-spin inline-block" />
                      ) : (
                        <GitBranch size={14} />
                      )}
                    </button>
                  </div>
                  {branchFetchError && (
                    <p className="text-xs text-[#C8102E] mt-1">{branchFetchError}</p>
                  )}
                </div>
              </div>
            )}

            {error && <p className="text-sm text-[#C8102E]">{error}</p>}

            <div className="flex gap-3 pt-2">
              <Button type="submit" loading={creating}>Create Project</Button>
              <Button type="button" variant="secondary" onClick={() => setShowCreate(false)}>Cancel</Button>
            </div>
          </form>
        </Card>
      )}

      {loading ? (
        <div className="flex items-center justify-center py-20 text-[#94A3B8]">Loading...</div>
      ) : projectList.length === 0 ? (
        <div className="text-center py-20">
          <FolderOpen size={48} className="text-[#E2E8F0] mx-auto mb-4" />
          <p className="text-[#64748B] font-medium">No projects yet</p>
          <p className="text-[#94A3B8] text-sm mt-1">Create a project to start analysing code with AI agents</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {projectList.map((p) => (
            <Card key={p.id}
              className="p-5 cursor-pointer hover:border-[#0050A0] hover:shadow-md transition-all group"
              onClick={() => router.push(`/projects/${p.id}`)}>
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-3 min-w-0">
                  <div className="w-9 h-9 rounded-lg bg-[#F1F5F9] flex items-center justify-center flex-shrink-0">
                    {p.source_type === "git" ? (
                      <GitBranch size={16} className="text-[#0050A0]" />
                    ) : (
                      <FolderOpen size={16} className="text-[#0050A0]" />
                    )}
                  </div>
                  <div className="min-w-0">
                    <h3 className="font-semibold text-[#1A1A2E] truncate group-hover:text-[#0050A0]">{p.name}</h3>
                    {p.description && <p className="text-sm text-[#64748B] truncate mt-0.5">{p.description}</p>}
                  </div>
                </div>
                <button onClick={(e) => handleDelete(p.id, e)}
                  className="opacity-0 group-hover:opacity-100 text-[#94A3B8] hover:text-[#C8102E] transition-all ml-2">
                  <Trash2 size={14} />
                </button>
              </div>
              <div className="flex items-center gap-1.5 mt-4 text-xs text-[#94A3B8]">
                <Clock size={11} />
                {formatDate(p.updated_at)}
                <span className="ml-2 px-2 py-0.5 rounded-full bg-[#F1F5F9] text-[#64748B] capitalize">
                  {p.source_type}
                </span>
                {p.source_type === "git" && p.default_branch && (
                  <span className="px-2 py-0.5 rounded-full bg-blue-50 text-[#0050A0]">
                    {p.default_branch}
                  </span>
                )}
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
