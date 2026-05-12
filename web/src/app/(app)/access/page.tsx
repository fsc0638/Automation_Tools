"use client";

import { type FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { Building2, FolderKey, ShieldCheck, Trash2, UserPlus } from "lucide-react";
import {
  organizations,
  projectAcl,
  projects,
  type AclMember,
  type OrgRole,
  type Organization,
  type Project,
  type ProjectRole,
  type Workspace,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, InlineBanner, SectionEmpty, SkeletonBlock } from "@/components/ui/card";
import { formatDate } from "@/lib/utils";
import { useToastStore } from "@/lib/toast-store";
import { useT } from "@/lib/i18n";

const ORG_ROLES: OrgRole[] = ["owner", "admin", "member", "viewer"];
const PROJECT_ROLES: ProjectRole[] = ["owner", "admin", "editor", "viewer"];

// Build the role-help map inside a hook so `t(...)` re-evaluates on locale
// change. (Module-level constants are frozen at module-eval time and would
// only render in the locale that was active during the first import.)
function useRoleHelp(): Record<OrgRole | ProjectRole, string> {
  const t = useT();
  return {
    owner: t("access.roleHelpOwner"),
    admin: t("access.roleHelpAdmin"),
    member: t("access.roleHelpMember"),
    editor: t("access.roleHelpEditor"),
    viewer: t("access.roleHelpViewer"),
  };
}

export default function AccessPage() {
  const pushToast = useToastStore((state) => state.pushToast);
  const t = useT();
  const [orgList, setOrgList] = useState<Organization[]>([]);
  const [workspaceList, setWorkspaceList] = useState<Workspace[]>([]);
  const [projectList, setProjectList] = useState<Project[]>([]);
  const [selectedOrgId, setSelectedOrgId] = useState("");
  const [selectedProjectId, setSelectedProjectId] = useState("");
  const [orgMembers, setOrgMembers] = useState<AclMember[]>([]);
  const [projectMembers, setProjectMembers] = useState<AclMember[]>([]);
  const [orgEmail, setOrgEmail] = useState("");
  const [orgRole, setOrgRole] = useState<OrgRole>("member");
  const [projectEmail, setProjectEmail] = useState("");
  const [projectRole, setProjectRole] = useState<ProjectRole>("viewer");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const selectedOrg = orgList.find((org) => org.id === selectedOrgId) ?? null;
  const selectedProject = projectList.find((project) => project.id === selectedProjectId) ?? null;

  const loadRoot = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const [orgs, projs] = await Promise.all([organizations.list(), projects.list()]);
      setOrgList(orgs);
      setProjectList(projs);
      setSelectedOrgId((current) => current || orgs[0]?.id || "");
      setSelectedProjectId((current) => current || projs[0]?.id || "");
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to load access settings";
      setError(message);
      pushToast({ tone: "error", title: "Access settings failed to load", description: message });
    } finally {
      setLoading(false);
    }
  }, [pushToast]);

  const loadOrgDetails = useCallback(async (orgId: string) => {
    try {
      const [workspaces, members] = await Promise.all([
        organizations.workspaces(orgId),
        organizations.members(orgId),
      ]);
      setWorkspaceList(workspaces);
      setOrgMembers(members);
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to load organization members";
      pushToast({ tone: "error", title: "Organization access failed", description: message });
    }
  }, [pushToast]);

  const loadProjectAcl = useCallback(async (projectId: string) => {
    try {
      setProjectMembers(await projectAcl.list(projectId));
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to load project ACL";
      pushToast({ tone: "error", title: "Project access failed", description: message });
    }
  }, [pushToast]);

  useEffect(() => {
    queueMicrotask(() => { void loadRoot(); });
  }, [loadRoot]);

  useEffect(() => {
    if (selectedOrgId) queueMicrotask(() => { void loadOrgDetails(selectedOrgId); });
  }, [loadOrgDetails, selectedOrgId]);

  useEffect(() => {
    if (selectedProjectId) queueMicrotask(() => { void loadProjectAcl(selectedProjectId); });
  }, [loadProjectAcl, selectedProjectId]);

  async function addOrgMember(e: FormEvent) {
    e.preventDefault();
    if (!selectedOrgId || !orgEmail.trim()) return;
    setBusy(true);
    try {
      const member = await organizations.addMember(selectedOrgId, { email: orgEmail.trim(), role: orgRole });
      setOrgMembers((items) => [member, ...items.filter((item) => item.user_id !== member.user_id)]);
      setOrgEmail("");
      pushToast({ tone: "success", title: "Organization member saved", description: `${member.email} is now ${member.role}.` });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to add member";
      pushToast({ tone: "error", title: "Could not add organization member", description: message });
    } finally {
      setBusy(false);
    }
  }

  async function addProjectMember(e: FormEvent) {
    e.preventDefault();
    if (!selectedProjectId || !projectEmail.trim()) return;
    setBusy(true);
    try {
      const member = await projectAcl.add(selectedProjectId, { email: projectEmail.trim(), role: projectRole });
      setProjectMembers((items) => [member, ...items.filter((item) => item.user_id !== member.user_id)]);
      setProjectEmail("");
      pushToast({ tone: "success", title: "Project access saved", description: `${member.email} is now ${member.role}.` });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to add project member";
      pushToast({ tone: "error", title: "Could not add project member", description: message });
    } finally {
      setBusy(false);
    }
  }

  async function updateOrgMember(userId: string, role: OrgRole) {
    if (!selectedOrgId) return;
    setBusy(true);
    try {
      const updated = await organizations.updateMember(selectedOrgId, userId, role);
      setOrgMembers((items) => items.map((item) => item.user_id === userId ? updated : item));
      pushToast({ tone: "success", title: "Role updated", description: `${updated.email} is now ${updated.role}.` });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to update role";
      pushToast({ tone: "error", title: "Could not update role", description: message });
    } finally {
      setBusy(false);
    }
  }

  async function updateProjectMember(userId: string, role: ProjectRole) {
    if (!selectedProjectId) return;
    setBusy(true);
    try {
      const updated = await projectAcl.update(selectedProjectId, userId, role);
      setProjectMembers((items) => items.map((item) => item.user_id === userId ? updated : item));
      pushToast({ tone: "success", title: "Project role updated", description: `${updated.email} is now ${updated.role}.` });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to update project role";
      pushToast({ tone: "error", title: "Could not update project role", description: message });
    } finally {
      setBusy(false);
    }
  }

  async function removeOrgMember(userId: string) {
    if (!selectedOrgId || !confirm("Remove this organization member?")) return;
    setBusy(true);
    try {
      await organizations.removeMember(selectedOrgId, userId);
      setOrgMembers((items) => items.filter((item) => item.user_id !== userId));
      pushToast({ tone: "warning", title: "Organization member removed" });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to remove member";
      pushToast({ tone: "error", title: "Could not remove member", description: message });
    } finally {
      setBusy(false);
    }
  }

  async function removeProjectMember(userId: string) {
    if (!selectedProjectId || !confirm("Remove this project member?")) return;
    setBusy(true);
    try {
      await projectAcl.remove(selectedProjectId, userId);
      setProjectMembers((items) => items.filter((item) => item.user_id !== userId));
      pushToast({ tone: "warning", title: "Project member removed" });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to remove project member";
      pushToast({ tone: "error", title: "Could not remove project member", description: message });
    } finally {
      setBusy(false);
    }
  }

  const workspaceSummary = useMemo(() => {
    if (!workspaceList.length) return "No workspaces";
    return `${workspaceList.length} workspace${workspaceList.length === 1 ? "" : "s"}`;
  }, [workspaceList.length]);

  return (
    <div className="mx-auto flex max-w-7xl flex-col gap-6 p-8">
      <section className="rounded-[28px] border border-[#E2E8F0] bg-white p-6 shadow-sm">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <div className="inline-flex items-center gap-2 rounded-full bg-[#EEF4FF] px-3 py-1 text-[12px] font-semibold tracking-[0.05em] text-[#0050A0]">
              <ShieldCheck size={13} /> {t("access.headerBadge")}
            </div>
            <h1 className="type-page-title mt-3">{t("access.title")}</h1>
            <p className="type-body-muted mt-2 max-w-2xl">{t("access.subtitle")}</p>
          </div>
          <Button onClick={() => void loadRoot()} disabled={loading || busy}>{t("access.refresh")}</Button>
        </div>
      </section>

      {error && <InlineBanner tone="error" title={t("access.unavailable")} description={error} />}

      {loading ? (
        <div className="grid gap-5 xl:grid-cols-2">
          <SkeletonBlock className="h-96" />
          <SkeletonBlock className="h-96" />
        </div>
      ) : (
        <div className="grid gap-5 xl:grid-cols-2">
          <Card className="p-5">
            <div className="flex items-start justify-between gap-3">
              <div>
                <div className="type-card-title flex items-center gap-2"><Building2 size={16} /> {t("access.orgAccessTitle")}</div>
                <p className="type-body-muted mt-1">{t("access.orgAccessDesc")}</p>
              </div>
              <span className="rounded-full bg-blue-50 px-3 py-1 text-[12px] font-semibold tracking-[0.05em] text-[#0050A0]">{workspaceSummary}</span>
            </div>

            <label className="type-overline mt-5 block">{t("access.orgLabel")}</label>
            <select
              value={selectedOrgId}
              onChange={(event) => setSelectedOrgId(event.target.value)}
              className="mt-2 w-full rounded-2xl border border-[#D6DFEA] bg-white px-4 py-3 text-[14px] leading-6 outline-none transition focus:border-[#0050A0] focus:ring-2 focus:ring-blue-100"
            >
              {orgList.map((org) => <option key={org.id} value={org.id}>{org.name} · {org.role}</option>)}
            </select>

            {selectedOrg && (
              <div className="type-meta mt-4 rounded-2xl border border-[#E2E8F0] bg-[#F8FAFC] p-4">
                <div className="text-[14px] font-medium leading-6 text-[#1A1A2E]">{selectedOrg.name}</div>
                <div className="mt-1">{t("access.yourRole")}: <span className="font-semibold text-[#0050A0]">{selectedOrg.role}</span></div>
                <div className="mt-1">{t("access.updatedAt")} {formatDate(selectedOrg.updated_at)}</div>
              </div>
            )}

            <form onSubmit={addOrgMember} className="mt-5 grid gap-3 rounded-2xl border border-[#E2E8F0] p-4 md:grid-cols-[1fr_150px_auto]">
              <Input placeholder={t("access.emailPlaceholder")} value={orgEmail} onChange={(event) => setOrgEmail(event.target.value)} />
              <RoleSelect roles={ORG_ROLES} value={orgRole} onChange={(role) => setOrgRole(role as OrgRole)} />
              <Button type="submit" disabled={busy || !selectedOrgId || !orgEmail.trim()}><UserPlus size={15} /> {t("access.addButton")}</Button>
            </form>

            <MemberList
              members={orgMembers}
              roles={ORG_ROLES}
              emptyTitle={t("access.noOrgMembers")}
              busy={busy}
              onRoleChange={(userId, role) => updateOrgMember(userId, role as OrgRole)}
              onRemove={removeOrgMember}
            />
          </Card>

          <Card className="p-5">
            <div className="flex items-start justify-between gap-3">
              <div>
                <div className="type-card-title flex items-center gap-2"><FolderKey size={16} /> {t("access.projectSharingTitle")}</div>
                <p className="type-body-muted mt-1">{t("access.projectSharingDesc")}</p>
              </div>
              <span className="rounded-full bg-emerald-50 px-3 py-1 text-[12px] font-semibold tracking-[0.05em] text-emerald-700">{projectList.length} {t("access.projectsCountSuffix")}</span>
            </div>

            <label className="type-overline mt-5 block">{t("access.projectLabel")}</label>
            <select
              value={selectedProjectId}
              onChange={(event) => setSelectedProjectId(event.target.value)}
              className="mt-2 w-full rounded-2xl border border-[#D6DFEA] bg-white px-4 py-3 text-[14px] leading-6 outline-none transition focus:border-[#0050A0] focus:ring-2 focus:ring-blue-100"
            >
              {projectList.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}
            </select>

            {selectedProject && (
              <div className="type-meta mt-4 rounded-2xl border border-[#E2E8F0] bg-[#F8FAFC] p-4">
                <div className="text-[14px] font-medium leading-6 text-[#1A1A2E]">{selectedProject.name}</div>
                <div className="mt-1">{t("access.sourceLabel")}: <span className="font-semibold">{selectedProject.source_type}</span></div>
                <div className="mt-1 truncate">{selectedProject.source_path}</div>
              </div>
            )}

            <form onSubmit={addProjectMember} className="mt-5 grid gap-3 rounded-2xl border border-[#E2E8F0] p-4 md:grid-cols-[1fr_150px_auto]">
              <Input placeholder={t("access.emailPlaceholder")} value={projectEmail} onChange={(event) => setProjectEmail(event.target.value)} />
              <RoleSelect roles={PROJECT_ROLES} value={projectRole} onChange={(role) => setProjectRole(role as ProjectRole)} />
              <Button type="submit" disabled={busy || !selectedProjectId || !projectEmail.trim()}><UserPlus size={15} /> {t("access.shareButton")}</Button>
            </form>

            <MemberList
              members={projectMembers}
              roles={PROJECT_ROLES}
              emptyTitle={t("access.noProjectMembers")}
              busy={busy}
              onRoleChange={(userId, role) => updateProjectMember(userId, role as ProjectRole)}
              onRemove={removeProjectMember}
            />
          </Card>
        </div>
      )}
    </div>
  );
}

function RoleSelect({ roles, value, onChange }: { roles: string[]; value: string; onChange: (role: string) => void }) {
  return (
    <select
      value={value}
      onChange={(event) => onChange(event.target.value)}
      className="rounded-2xl border border-[#D6DFEA] bg-white px-4 py-3 text-[14px] leading-6 outline-none transition focus:border-[#0050A0] focus:ring-2 focus:ring-blue-100"
    >
      {roles.map((role) => <option key={role} value={role}>{role}</option>)}
    </select>
  );
}

function MemberList({
  members,
  roles,
  emptyTitle,
  busy,
  onRoleChange,
  onRemove,
}: {
  members: AclMember[];
  roles: string[];
  emptyTitle: string;
  busy: boolean;
  onRoleChange: (userId: string, role: string) => void;
  onRemove: (userId: string) => void;
}) {
  const t = useT();
  const roleHelp = useRoleHelp();
  if (!members.length) {
    return <SectionEmpty title={emptyTitle} description={t("access.addFirstHint")} />;
  }

  return (
    <div className="mt-5 overflow-hidden rounded-2xl border border-[#E2E8F0]">
      {members.map((member) => (
        <div key={member.user_id} className="grid gap-3 border-b border-[#E2E8F0] p-4 last:border-b-0 md:grid-cols-[1fr_150px_auto] md:items-center">
          <div className="min-w-0">
            <div className="truncate text-[14px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">{member.display_name || member.email}</div>
            <div className="mt-1 truncate text-[12px] leading-5 text-[#64748B]">{member.email}</div>
            <div className="mt-2 text-[12px] leading-5 text-[#94A3B8]">{roleHelp[member.role]} · {t("access.addedAt")} {formatDate(member.created_at)}</div>
          </div>
          <RoleSelect roles={roles} value={member.role} onChange={(role) => onRoleChange(member.user_id, role)} />
          <Button variant="ghost" disabled={busy} onClick={() => onRemove(member.user_id)}>
            <Trash2 size={15} /> {t("access.removeButton")}
          </Button>
        </div>
      ))}
    </div>
  );
}
