"use client";
import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Plus, GitBranch, FolderOpen, Trash2, Clock } from "lucide-react";
import { projects as projectsApi, type Project } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card } from "@/components/ui/card";
import { formatDate } from "@/lib/utils";

export default function ProjectsPage() {
  const router = useRouter();
  const [projectList, setProjectList] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState({ name: "", description: "", source_type: "local", source_path: "" });
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => { load(); }, []);

  async function load() {
    try {
      setProjectList(await projectsApi.list());
    } finally {
      setLoading(false);
    }
  }

  async function handleCreate(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setCreating(true);
    try {
      await projectsApi.create(form);
      setShowCreate(false);
      setForm({ name: "", description: "", source_type: "local", source_path: "" });
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create project");
    } finally {
      setCreating(false);
    }
  }

  async function handleDelete(id: string, e: React.MouseEvent) {
    e.stopPropagation();
    if (!confirm("Delete this project?")) return;
    await projectsApi.delete(id);
    await load();
  }

  return (
    <div className="p-8 max-w-5xl mx-auto">
      {/* Header */}
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold text-[#1A1A2E]">Projects</h1>
          <p className="text-[#64748B] text-sm mt-1">Manage your code repositories and local projects</p>
        </div>
        <Button onClick={() => setShowCreate(true)}>
          <Plus size={16} />
          New Project
        </Button>
      </div>

      {/* Create form */}
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
              onChange={(e) => setForm((f) => ({ ...f, source_path: e.target.value }))} required />

            {error && <p className="text-sm text-[#C8102E]">{error}</p>}

            <div className="flex gap-3 pt-2">
              <Button type="submit" loading={creating}>Create Project</Button>
              <Button type="button" variant="secondary" onClick={() => setShowCreate(false)}>Cancel</Button>
            </div>
          </form>
        </Card>
      )}

      {/* List */}
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
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
