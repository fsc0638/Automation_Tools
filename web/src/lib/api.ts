const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:8080/api";

function getToken(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem("kway_token");
}

function getRefreshToken(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem("kway_refresh_token");
}

function setSession(accessToken: string, refreshToken?: string) {
  if (typeof window === "undefined") return;
  localStorage.setItem("kway_token", accessToken);
  if (refreshToken) localStorage.setItem("kway_refresh_token", refreshToken);
}

function clearSession() {
  if (typeof window === "undefined") return;
  localStorage.removeItem("kway_token");
  localStorage.removeItem("kway_refresh_token");
}

/** Single in-flight refresh — concurrent 401s share one fetch. */
let refreshInFlight: Promise<boolean> | null = null;

async function tryRefresh(): Promise<boolean> {
  const refreshToken = getRefreshToken();
  if (!refreshToken) return false;
  if (refreshInFlight) return refreshInFlight;
  refreshInFlight = (async () => {
    try {
      const res = await fetch(`${API_BASE}/auth/refresh`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ refresh_token: refreshToken }),
      });
      if (!res.ok) return false;
      const data: { access_token: string; refresh_token: string } = await res.json();
      setSession(data.access_token, data.refresh_token);
      return true;
    } catch {
      return false;
    } finally {
      refreshInFlight = null;
    }
  })();
  return refreshInFlight;
}

async function request<T>(path: string, options: RequestInit = {}, retry = true): Promise<T> {
  const token = getToken();
  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...options.headers,
    },
  });

  // 401 path: try the refresh token once. If refresh succeeds, replay
  // the original request transparently. If refresh fails (no refresh
  // token, expired, server reject), fall through to the legacy logout
  // behaviour below. Login/register endpoints skip this path entirely
  // so a wrong-password 401 stays as a form error.
  if (res.status === 401 && token && typeof window !== "undefined" && retry) {
    const currentPath = window.location.pathname;
    const isAuthEndpoint = path.startsWith("/auth/");
    if (!isAuthEndpoint && currentPath !== "/login" && currentPath !== "/register") {
      const refreshed = await tryRefresh();
      if (refreshed) {
        // Replay the original request with the new access token.
        return request<T>(path, options, false);
      }
      clearSession();
      window.location.replace("/login");
      return new Promise<T>(() => {});
    }
  }

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(err.error ?? "Request failed");
  }
  if (res.status === 204) return undefined as T;
  return res.json();
}

// Auth
export const auth = {
  register: (data: { email: string; password: string; display_name: string }) =>
    request<AuthResponse>("/auth/register", {
      method: "POST",
      body: JSON.stringify(data),
    }),
  login: (data: { email: string; password: string }) =>
    request<AuthResponse>("/auth/login", {
      method: "POST",
      body: JSON.stringify(data),
    }),
  logout: () => {
    const refreshToken = getRefreshToken();
    clearSession();
    if (!refreshToken) return Promise.resolve();
    // Best-effort server-side invalidation. Even if this fails (network
    // error, etc.) the local session is already gone.
    return fetch(`${API_BASE}/auth/logout`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: refreshToken }),
    }).catch(() => undefined);
  },
};

export interface AuthResponse {
  access_token: string;
  refresh_token: string;
  token_type: string;
  refresh_expires_in: number;
  user: UserInfo;
}

// Projects
export const projects = {
  list: () => request<Project[]>("/projects"),
  create: (data: CreateProjectInput) =>
    request<Project>("/projects", { method: "POST", body: JSON.stringify(data) }),
  /** Phase 4: soft 封存/取消封存 (sets/clears archived_at). */
  archive: (id: string, archived: boolean) =>
    request<Project>(`/projects/${id}/archive`, {
      method: "POST",
      body: JSON.stringify({ archived }),
    }),
  upload: async (data: { name: string; description?: string; file: File }) => {
    const token = getToken();
    const form = new FormData();
    form.append("name", data.name);
    if (data.description) form.append("description", data.description);
    form.append("file", data.file);
    const res = await fetch(`${API_BASE}/projects/upload`, {
      method: "POST",
      headers: {
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: form,
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: res.statusText }));
      throw new Error(err.error ?? "Upload failed");
    }
    return res.json() as Promise<Project>;
  },
  get: (id: string) => request<Project>(`/projects/${id}`),
  delete: (id: string) => request<void>(`/projects/${id}`, { method: "DELETE" }),
  fileTree: (id: string) => request<FileNode[]>(`/projects/${id}/files`),
  fileContent: (id: string, path: string) =>
    request<{ path: string; content: string }>(`/projects/${id}/files/content?path=${encodeURIComponent(path)}`),
  gitStatus: (id: string) => request<GitStatus>(`/projects/${id}/git/status`),
  gitBranches: (id: string) => request<{ branches: string[] }>(`/projects/${id}/git/branches`),
  checkoutBranch: (id: string, branch: string) =>
    request<Project>(`/projects/${id}/git/checkout`, { method: "POST", body: JSON.stringify({ branch }) }),
  gitSync: (id: string) =>
    request<{ status: "up-to-date" | "fast-forwarded" | "no-remote-branch" }>(
      `/projects/${id}/git/sync`,
      { method: "POST" },
    ),
  reindex: (id: string) =>
    request<{ indexed_files: number }>(`/projects/${id}/index`, { method: "POST" }),
  metricsSummary: (id: string) =>
    request<MetricsSummary>(`/projects/${id}/metrics/summary`),
  metricsCost: (id: string) =>
    request<MetricsCost>(`/projects/${id}/metrics/cost`),
  metricsBurndown: (id: string, opts?: { sprintId?: string; days?: number }) => {
    const params = new URLSearchParams();
    if (opts?.sprintId) params.set("sprint_id", opts.sprintId);
    if (opts?.days) params.set("days", String(opts.days));
    const qs = params.toString();
    return request<MetricsBurndown>(`/projects/${id}/metrics/burndown${qs ? `?${qs}` : ""}`);
  },
  metricsHealth: (id: string) =>
    request<MetricsHealth>(`/projects/${id}/metrics/health`),
  remoteBranches: (url: string, git_identity_id?: string) =>
    request<{ branches: string[] }>("/git/remote-branches", {
      method: "POST",
      body: JSON.stringify({ url, git_identity_id: git_identity_id ?? null }),
    }),
};

export const organizations = {
  list: () => request<Organization[]>("/organizations"),
  workspaces: (organizationId: string) =>
    request<Workspace[]>(`/organizations/${organizationId}/workspaces`),
  members: (organizationId: string) =>
    request<AclMember[]>(`/organizations/${organizationId}/members`),
  addMember: (organizationId: string, data: { email: string; role: OrgRole }) =>
    request<AclMember>(`/organizations/${organizationId}/members`, {
      method: "POST",
      body: JSON.stringify(data),
    }),
  updateMember: (organizationId: string, userId: string, role: OrgRole) =>
    request<AclMember>(`/organizations/${organizationId}/members/${userId}`, {
      method: "PATCH",
      body: JSON.stringify({ role }),
    }),
  removeMember: (organizationId: string, userId: string) =>
    request<void>(`/organizations/${organizationId}/members/${userId}`, { method: "DELETE" }),
};

export const projectAcl = {
  list: (projectId: string) => request<AclMember[]>(`/projects/${projectId}/acl`),
  add: (projectId: string, data: { email: string; role: ProjectRole }) =>
    request<AclMember>(`/projects/${projectId}/acl`, {
      method: "POST",
      body: JSON.stringify(data),
    }),
  update: (projectId: string, userId: string, role: ProjectRole) =>
    request<AclMember>(`/projects/${projectId}/acl/${userId}`, {
      method: "PATCH",
      body: JSON.stringify({ role }),
    }),
  remove: (projectId: string, userId: string) =>
    request<void>(`/projects/${projectId}/acl/${userId}`, { method: "DELETE" }),
};

export const feedback = {
  submit: (messageId: string, rating: 1 | -1, note?: string) =>
    request<{ id: string; message_id: string; rating: number; note: string | null }>(
      `/messages/${messageId}/feedback`,
      { method: "POST", body: JSON.stringify({ rating, note }) },
    ),
};

export const tasks = {
  list: (projectId: string, opts?: { sprintId?: string | "none" }) => {
    const params = opts?.sprintId ? `?sprint_id=${encodeURIComponent(opts.sprintId)}` : "";
    return request<ProjectTask[]>(`/projects/${projectId}/tasks${params}`);
  },
  create: (projectId: string, data: CreateTaskInput) =>
    request<ProjectTask>(`/projects/${projectId}/tasks`, {
      method: "POST",
      body: JSON.stringify(data),
    }),
  update: (projectId: string, taskId: string, data: UpdateTaskInput) =>
    request<ProjectTask>(`/projects/${projectId}/tasks/${taskId}`, {
      method: "PATCH",
      body: JSON.stringify(data),
    }),
  delete: (projectId: string, taskId: string) =>
    request<void>(`/projects/${projectId}/tasks/${taskId}`, { method: "DELETE" }),
  history: (projectId: string, taskId: string) =>
    request<TaskStatusEvent[]>(`/projects/${projectId}/tasks/${taskId}/history`),
  attempts: (projectId: string, taskId: string) =>
    request<TaskAttempt[]>(`/projects/${projectId}/tasks/${taskId}/attempts`),
  dispatch: (projectId: string, taskId: string, data: DispatchTaskInput) =>
    request<DispatchTaskResult>(`/projects/${projectId}/tasks/${taskId}/attempts`, {
      method: "POST",
      body: JSON.stringify(data),
    }),
  comments: (projectId: string, taskId: string) =>
    request<TaskComment[]>(`/projects/${projectId}/tasks/${taskId}/comments`),
  addComment: (projectId: string, taskId: string, content: string) =>
    request<TaskComment>(`/projects/${projectId}/tasks/${taskId}/comments`, {
      method: "POST",
      body: JSON.stringify({ content }),
    }),
  updateComment: (projectId: string, taskId: string, commentId: string, content: string) =>
    request<TaskComment>(`/projects/${projectId}/tasks/${taskId}/comments/${commentId}`, {
      method: "PATCH",
      body: JSON.stringify({ content }),
    }),
  deleteComment: (projectId: string, taskId: string, commentId: string) =>
    request<void>(`/projects/${projectId}/tasks/${taskId}/comments/${commentId}`, {
      method: "DELETE",
    }),
};

export const gitIdentities = {
  list: () => request<GitIdentity[]>("/git/identities"),
  create: (data: { name: string; provider?: string; username: string; access_token: string; repository_url?: string }) =>
    request<GitIdentity>("/git/identities", { method: "POST", body: JSON.stringify(data) }),
  delete: (id: string) => request<void>(`/git/identities/${id}`, { method: "DELETE" }),
};

export const agentProfiles = {
  list: () => request<AgentProfile[]>("/agents"),
  create: (data: CreateAgentProfileInput) =>
    request<AgentProfile>("/agents", { method: "POST", body: JSON.stringify(data) }),
  update: (id: string, data: UpdateAgentProfileInput) =>
    request<AgentProfile>(`/agents/${id}`, { method: "PATCH", body: JSON.stringify(data) }),
  delete: (id: string) => request<void>(`/agents/${id}`, { method: "DELETE" }),
};

export const projectMemory = {
  candidates: (projectId: string, status: "pending" | "approved" | "rejected" | "all" = "pending") =>
    request<ProjectMemoryCandidate[]>(`/projects/${projectId}/memory/candidates?status=${status}`),
  approveCandidate: (projectId: string, candidateId: string, review_note?: string) =>
    request<ProjectMemoryCandidate>(`/projects/${projectId}/memory/candidates/${candidateId}/approve`, {
      method: "POST",
      body: JSON.stringify({ review_note }),
    }),
  rejectCandidate: (projectId: string, candidateId: string, review_note?: string) =>
    request<void>(`/projects/${projectId}/memory/candidates/${candidateId}/reject`, {
      method: "POST",
      body: JSON.stringify({ review_note }),
    }),
  bulkApproveCandidates: (projectId: string, ids: string[]) =>
    request<void>(`/projects/${projectId}/memory/candidates/bulk-approve`, {
      method: "POST",
      body: JSON.stringify({ ids }),
    }),
  bulkRejectCandidates: (projectId: string, ids: string[]) =>
    request<void>(`/projects/${projectId}/memory/candidates/bulk-reject`, {
      method: "POST",
      body: JSON.stringify({ ids }),
    }),
};

// ─────────────────────────────────────────────────────────────────────
// Meetings
// ─────────────────────────────────────────────────────────────────────

export type MeetingImportance = "normal" | "important";
export type MeetingRecurrence = "none" | "daily" | "weekly" | "monthly";
export type MeetingStatus =
  | "draft"
  | "scheduled"
  | "in_progress"
  | "completed"
  | "cancelled";
export type MeetingFileCategory = "attachment" | "recording" | "transcript";
export type MeetingImpactType = "new" | "update" | "progress";
export type AttendeeStatus = "pending" | "confirmed" | "disputed";

export interface Meeting {
  id: string;
  creator_id: string;
  organization_id: string | null;
  project_id: string | null;
  title: string;
  importance: MeetingImportance;
  start_at: string;
  end_at: string;
  all_day: boolean;
  recurrence: MeetingRecurrence;
  timezone: string;
  location: string | null;
  notification_note: string | null;
  status: MeetingStatus;
  invitations_sent_at: string | null;
  /** Display name of the user who created this meeting. Populated by the
   *  list endpoint via a JOIN against `users`; absent on detail responses
   *  so callers must tolerate `undefined`. */
  creator_name?: string;
  /** When the meeting was successfully pushed to crm.kway.com.tw's
   *  reservation form. Undefined while still local-only or after a
   *  portal cancel. */
  portal_booked_at?: string;
  /** Last portal-side failure (cleared on success). Surfaced as a red
   *  banner on the detail page so the operator can retry. */
  portal_book_error?: string;

  // AgentK-aligned optional fields (migration 0032).
  /** Long-form description; distinct from notification_note. */
  description?: string;
  /** Independent lock flag. True ⇒ no further edits; reopen clears. */
  is_locked?: boolean;
  /** Online meeting URL (Webex / Teams / Meet). */
  join_url?: string;
  /** Symbolic provider (`webex` / `teams` / `meet` / `kway-portal`). */
  external_provider?: string;
  external_event_id?: string;
  external_event_url?: string;
  sync_status?: string;
  last_synced_at?: string;
  updated_by_user_id?: string;
  /** AgentK-aligned busy masking. `"full"` = caller is a participant
   *  (creator / attendee) and sees all fields; `"busy"` = caller can
   *  only see this row as occupancy — title is `"(忙碌)"` and
   *  notification_note / description / join_url are blanked. Absent on
   *  detail responses. */
  visibility?: "full" | "busy";
  created_at: string;
  updated_at: string;
}

export interface MeetingAttendee {
  meeting_id: string;
  user_id: string | null;
  email: string;
  display_name: string;
  role_label: string | null;
  confirmation_status: AttendeeStatus;
  confirmed_at: string | null;
  dispute_note: string | null;
  last_action_at: string | null;
  created_at: string;
}

export interface MeetingFile {
  id: string;
  meeting_id: string;
  uploader_id: string;
  filename: string;
  storage_path: string;
  file_size: number;
  mime_type: string;
  file_category: MeetingFileCategory;
  upload_status: "pending" | "uploaded" | "processing" | "failed";
  duration_seconds: number | null;
  transcript_meta: string | null;
  created_at: string;
  // AgentK-aligned soft-delete retention (migration 0034). Present only
  // on soft-deleted files. Default file-list endpoint hides those, so
  // these fields are mostly relevant for a future recovery view.
  deleted_at?: string;
  soft_deleted_until?: string;
  hard_delete_after?: string;
  metadata?: unknown;
}

export interface ReconcileProposal {
  title: string;
  description: string;
  /** AI suggestion: new | continue | duplicate */
  suggested: "new" | "continue" | "duplicate";
  target_task_id?: string;
  target_task_title?: string;
  target_task_status?: string;
  reason?: string;
}

export interface SyncPreviewResult {
  notes_version: number;
  proposals: ReconcileProposal[];
}

export interface SyncDecisionInput {
  title: string;
  /** new | continue | skip */
  decision: "new" | "continue" | "skip";
  target_task_id?: string;
}

export interface SyncTasksResult {
  synced_notes_version: number;
  created_task_ids: string[];
  linked_task_ids: string[];
  skipped_existing_titles: string[];
}

export interface ProjectMeetingActionItem {
  title: string;
  description?: string;
  assignee_name?: string;
  /** Present when the action item title matched a project_task. */
  task_id?: string;
  /** Live status of that task: todo / in_progress / done / ... */
  task_status?: string;
}

export interface ProjectMeetingHistoryItem {
  meeting_id: string;
  title: string;
  start_at: string;
  end_at: string;
  status: MeetingStatus;
  is_locked: boolean;
  creator_name?: string;
  summary?: string;
  decisions: Array<{ text: string; resolved?: boolean }>;
  action_items: ProjectMeetingActionItem[];
}

export interface MeetingActionItem {
  title: string;
  description?: string;
  /** Resolved user id when AI-generated assignee matched a real user. */
  assignee_user_id?: string;
  /** Raw assignee name from the LLM (kept verbatim — UI shows this
   *  when assignee_user_id couldn't be resolved). */
  assignee_name?: string;
  source?: string;
}

export interface MeetingNotes {
  id: string;
  meeting_id: string;
  version: number;
  summary: string | null;
  decisions: Array<{ text: string; resolved?: boolean }>;
  risks: Array<{ text: string; severity?: "low" | "medium" | "high" }>;
  transcript_excerpts: Array<{ speaker: string; time: string; content: string }>;
  generated_by: string;
  created_at: string;
  // AgentK-aligned record aggregate fields (migration 0033).
  action_items: MeetingActionItem[];
  ai_job_ids: string[];
  task_ids: string[];
}

export interface MeetingTaskImpact {
  id: string;
  meeting_id: string;
  project_id: string | null;
  task_id: string | null;
  impact_type: MeetingImpactType;
  description: string;
  progress_from: number | null;
  progress_to: number | null;
  is_hidden: boolean;
  created_at: string;
}

export interface MeetingDetail extends Meeting {
  attendees: MeetingAttendee[];
  files: MeetingFile[];
  latest_notes: MeetingNotes | null;
  task_impacts: MeetingTaskImpact[];
  linked_project: { id: string; name: string } | null;
}

export interface MeetingCalendarDay {
  date: string;
  meeting_count: number;
  has_urgent: boolean;
  has_available_slot: boolean;
}

export interface MeetingTimeSlot {
  start_at: string;
  end_at: string;
  available_count: number;
  total_attendees: number;
  busy_names: string[];
}

export interface MeetingNotesEdit {
  id: string;
  meeting_id: string;
  version: number;
  edited_by: string;
  editor_name: string | null;
  edit_summary: string;
  snapshot: unknown;
  created_at: string;
}

export interface CreateMeetingInput {
  title: string;
  importance?: MeetingImportance;
  start_at: string;
  end_at: string;
  all_day?: boolean;
  recurrence?: MeetingRecurrence;
  timezone?: string;
  location?: string | null;
  notification_note?: string | null;
  attendee_emails?: string[];
  project_id?: string | null;
  save_as_draft?: boolean;
  // AgentK-aligned optional fields. All omittable — backend tolerates
  // missing keys.
  description?: string | null;
  join_url?: string | null;
  external_provider?: string | null;
  external_event_id?: string | null;
  external_event_url?: string | null;
}

export interface UpdateMeetingInput {
  title?: string;
  importance?: MeetingImportance;
  start_at?: string;
  end_at?: string;
  all_day?: boolean;
  recurrence?: MeetingRecurrence;
  timezone?: string;
  location?: string | null;
  notification_note?: string | null;
  status?: MeetingStatus;
  attendee_emails?: string[];
  project_id?: string | null;
  // AgentK-aligned. is_locked is settable but reopen flow should
  // normally go through its dedicated endpoint (role-guarded).
  description?: string | null;
  join_url?: string | null;
  external_provider?: string | null;
  external_event_id?: string | null;
  external_event_url?: string | null;
  is_locked?: boolean;
}

export interface MeetingSyncReport {
  range_start: string;
  range_end: string;
  stdout_tail: string;
  inserted: number;
  updated: number;
  unchanged: number;
  cancelled: number;
  skipped: number;
  elapsed_ms: number;
}

export const meetings = {
  sync: () => request<MeetingSyncReport>("/meetings/sync", { method: "POST" }),
  list: (q: {
    projectId?: string;
    status?: MeetingStatus;
    from?: string;
    to?: string;
  } = {}) => {
    const p = new URLSearchParams();
    if (q.projectId) p.set("project_id", q.projectId);
    if (q.status) p.set("status", q.status);
    if (q.from) p.set("from", q.from);
    if (q.to) p.set("to", q.to);
    const qs = p.toString();
    return request<Meeting[]>(`/meetings${qs ? `?${qs}` : ""}`);
  },
  create: (data: CreateMeetingInput) =>
    request<MeetingDetail>("/meetings", { method: "POST", body: JSON.stringify(data) }),
  get: (id: string) => request<MeetingDetail>(`/meetings/${id}`),
  update: (id: string, data: UpdateMeetingInput) =>
    request<MeetingDetail>(`/meetings/${id}`, { method: "PATCH", body: JSON.stringify(data) }),
  delete: (id: string) => request<void>(`/meetings/${id}`, { method: "DELETE" }),
  sendInvitations: (id: string) =>
    request<MeetingDetail>(`/meetings/${id}/send-invitations`, { method: "POST" }),
  reopen: (id: string) =>
    request<MeetingDetail>(`/meetings/${id}/reopen`, { method: "POST" }),
  calendar: (year: number, month: number) =>
    request<MeetingCalendarDay[]>(`/meetings/calendar?year=${year}&month=${month}`),
  availableSlots: (date: string, durationMins: number, emails: string[] = []) => {
    const p = new URLSearchParams({ date, duration_mins: String(durationMins) });
    if (emails.length > 0) p.set("emails", emails.join(","));
    return request<MeetingTimeSlot[]>(`/meetings/available-slots?${p.toString()}`);
  },
  confirmAttendance: (id: string, email: string) =>
    request<MeetingAttendee>(
      `/meetings/${id}/attendees/${encodeURIComponent(email)}/confirm`,
      { method: "PATCH" }
    ),
  disputeAttendance: (id: string, email: string, note?: string) =>
    request<MeetingAttendee>(
      `/meetings/${id}/attendees/${encodeURIComponent(email)}/dispute`,
      { method: "PATCH", body: JSON.stringify({ note }) }
    ),
  uploadFile: async (
    id: string,
    file: File,
    opts: { category?: MeetingFileCategory; durationSeconds?: number; transcriptMeta?: string } = {}
  ) => {
    const token = getToken();
    const form = new FormData();
    form.append("file", file);
    form.append("category", opts.category ?? "attachment");
    if (opts.durationSeconds != null) form.append("duration_seconds", String(opts.durationSeconds));
    if (opts.transcriptMeta) form.append("transcript_meta", opts.transcriptMeta);
    const res = await fetch(`${API_BASE}/meetings/${id}/files`, {
      method: "POST",
      headers: token ? { Authorization: `Bearer ${token}` } : {},
      body: form,
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: res.statusText }));
      throw new Error(err.error ?? "Upload failed");
    }
    return (await res.json()) as MeetingFile;
  },
  deleteFile: (id: string, fileId: string) =>
    request<void>(`/meetings/${id}/files/${fileId}`, { method: "DELETE" }),
  generateNotes: (id: string) =>
    request<MeetingNotes>(`/meetings/${id}/notes/generate`, { method: "POST" }),
  /** Step 1: AI-reconcile the latest notes' action items against the
   *  project's existing tasks. No DB writes — returns a proposal the
   *  user confirms. */
  syncTasksPreview: (id: string) =>
    request<SyncPreviewResult>(`/meetings/${id}/notes/sync-tasks/preview`, {
      method: "POST",
    }),
  /** Step 2: apply the user-confirmed decisions. Omit `decisions` to
   *  fall back to legacy casefold auto-create. */
  syncNotesToTasks: (id: string, decisions?: SyncDecisionInput[]) =>
    request<SyncTasksResult>(`/meetings/${id}/notes/sync-tasks`, {
      method: "POST",
      ...(decisions ? { body: JSON.stringify({ decisions }) } : {}),
    }),
  projectMeetingHistory: (projectId: string) =>
    request<ProjectMeetingHistoryItem[]>(`/projects/${projectId}/meeting-history`),
  updateNotes: (
    id: string,
    body: Partial<Pick<MeetingNotes,
      "summary" | "decisions" | "risks" | "transcript_excerpts" |
      "action_items" | "ai_job_ids" | "task_ids">> & {
      edit_summary?: string;
    }
  ) => request<MeetingNotes>(`/meetings/${id}/notes`, { method: "PATCH", body: JSON.stringify(body) }),
  notesHistory: (id: string) =>
    request<MeetingNotesEdit[]>(`/meetings/${id}/notes/history`),
  addTaskImpact: (
    id: string,
    body: {
      project_id?: string | null;
      task_id?: string | null;
      impact_type: MeetingImpactType;
      description: string;
      progress_from?: number;
      progress_to?: number;
      is_hidden?: boolean;
    }
  ) =>
    request<MeetingTaskImpact>(`/meetings/${id}/task-impacts`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  deleteTaskImpact: (id: string, impactId: string) =>
    request<void>(`/meetings/${id}/task-impacts/${impactId}`, { method: "DELETE" }),
  roomsAvailable: (startAt: string, endAt: string) => {
    const p = new URLSearchParams({ start_at: startAt, end_at: endAt });
    return request<RoomAvailability[]>(`/meetings/rooms/available?${p.toString()}`);
  },
};

// Portal directory (employees + departments)
export interface RoomAvailability {
  name: string;
  available: boolean;
  conflict_title?: string;
  conflict_start_at?: string;
  conflict_end_at?: string;
}

export interface PortalDepartment {
  code: string;
  name: string;
}

export interface PortalEmployee {
  employee_no: string;
  name: string;
  email: string | null;
  title: string | null;
  dept_code: string | null;
  dept_name: string | null;
  extensions: string[];
}

export const portalDirectory = {
  departments: () => request<PortalDepartment[]>("/portal/departments"),
  searchEmployees: (params: { q?: string; deptCode?: string; limit?: number } = {}) => {
    const p = new URLSearchParams();
    if (params.q) p.set("q", params.q);
    if (params.deptCode) p.set("dept_code", params.deptCode);
    if (params.limit != null) p.set("limit", String(params.limit));
    const qs = p.toString();
    return request<PortalEmployee[]>(`/portal/employees/search${qs ? `?${qs}` : ""}`);
  },
};

// Conversations
export const conversations = {
  list: (projectId: string, mode?: AgentMode) =>
    request<Conversation[]>(`/projects/${projectId}/conversations${mode ? `?mode=${mode}` : ""}`),
  create: (projectId: string, title?: string, mode?: ChatMode) =>
    request<Conversation>(`/projects/${projectId}/conversations`, {
      method: "POST",
      body: JSON.stringify({ title, mode }),
    }),
  get: (projectId: string, convId: string) =>
    request<ConversationWithMessages>(`/projects/${projectId}/conversations/${convId}`),
  sendMessage: (projectId: string, convId: string, data: { content: string; file_path?: string; mode?: string }) =>
    request<{ messages: Message[] }>(`/projects/${projectId}/conversations/${convId}/messages`, {
      method: "POST",
      body: JSON.stringify(data),
    }),
  delete: (projectId: string, convId: string) =>
    request<void>(`/projects/${projectId}/conversations/${convId}`, { method: "DELETE" }),
  /**
   * Fetch the cached per-conversation summary. Null when the conversation
   * is too new to have a summary yet (refreshed asynchronously after each
   * turn server-side).
   */
  summary: (projectId: string, convId: string) =>
    request<ConversationSummary | null>(
      `/projects/${projectId}/conversations/${convId}/summary`,
    ),
};

export interface ConversationSummary {
  conversation_id: string;
  summary: string;
  highlights: string[];
  keywords: string[];
  source_message_count: number;
  updated_at: string;
}

/** AgentK-aligned: subscribe to meeting lifecycle events. Server pushes
 *  JSON like `{type:"updated",meeting_id:"..."}`. The handler is fired
 *  per event; lagged subscribers receive `{type:"resync"}` and should
 *  refetch from scratch. Returns the WebSocket so the caller can close
 *  it on unmount. */
export function createMeetingsWsConnection(
  onEvent: (event: { type: string; meeting_id?: string; is_locked?: boolean }) => void
): WebSocket {
  const token = getToken();
  const apiUrl = new URL(process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:8080/api");
  const wsProtocol = apiUrl.protocol === "https:" ? "wss:" : "ws:";
  const params = new URLSearchParams({ ...(token ? { token } : {}) });
  const ws = new WebSocket(
    `${wsProtocol}//${apiUrl.host}${apiUrl.pathname.replace(/\/$/, "")}/ws/meetings?${params}`
  );
  ws.onmessage = (e) => {
    try {
      onEvent(JSON.parse(e.data));
    } catch {
      /* ignore malformed payloads */
    }
  };
  return ws;
}

export function createWsConnection(conversationId: string, projectId: string): WebSocket {
  const token = getToken();
  const apiUrl = new URL(process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:8080/api");
  const wsProtocol = apiUrl.protocol === "https:" ? "wss:" : "ws:";
  const params = new URLSearchParams({
    ...(token ? { token } : {}),
    conversation_id: conversationId,
    project_id: projectId,
  });
  return new WebSocket(`${wsProtocol}//${apiUrl.host}/api/ws/chat?${params.toString()}`);
}

// Types
export interface UserInfo {
  id: string;
  email: string;
  display_name: string;
}

export type WorkspaceKind = "code" | "admin" | "general";

export interface CreateProjectInput {
  name: string;
  description?: string;
  source_type: string;
  source_path: string;
  git_identity_id?: string;
  default_branch?: string;
  /** Phase 2/3: omit ⇒ "code" (repo-backed). admin/general = 行政庶務. */
  kind?: WorkspaceKind;
}

export interface Project {
  id: string;
  user_id: string;
  organization_id: string;
  workspace_id: string;
  name: string;
  description?: string;
  source_type: string;
  source_path: string;
  local_path?: string;
  default_branch?: string;
  git_identity_id?: string;
  /** Workspace kind (migration 0038). "code" = repo-backed (default);
   *  "admin"/"general" = 行政庶務 work area with no repo. */
  kind?: WorkspaceKind;
  /** Soft archive marker; null/absent = active. */
  archived_at?: string | null;
  /** Effective ACL role for the current user. Populated by list/get endpoints. */
  effective_role?: ProjectRole | null;
  created_at: string;
  updated_at: string;
}

export type OrgRole = "owner" | "admin" | "member" | "viewer";
export type ProjectRole = "owner" | "admin" | "editor" | "viewer";

export interface AclMember {
  user_id: string;
  email: string;
  display_name: string;
  role: OrgRole | ProjectRole;
  created_at: string;
}

export interface Organization {
  id: string;
  name: string;
  owner_user_id: string;
  role?: OrgRole | null;
  created_at: string;
  updated_at: string;
}

export interface Workspace {
  id: string;
  organization_id: string;
  name: string;
  role?: OrgRole | null;
  created_at: string;
  updated_at: string;
}

export interface GitIdentity {
  id: string;
  user_id: string;
  name: string;
  provider: string;
  username: string;
  created_at: string;
  updated_at: string;
}

export interface FileNode {
  name: string;
  path: string;
  is_dir: boolean;
  children?: FileNode[];
}

export interface GitStatus {
  branch: string;
  changed: string[];
  staged: string[];
  untracked: string[];
}

export type AgentMode = "openclaw" | "hermes" | "debate";
export type ChatMode = AgentMode | `agent:${string}` | `agents:${string}`;

export interface AgentProfile {
  id: string;
  user_id: string;
  name: string;
  provider: "openai" | "openai_compatible" | "gemini" | "anthropic";
  model: string;
  base_url?: string | null;
  role_prompt: string;
  enabled: boolean;
  /** B7: free-form labels for grouping agents on the /agents page. */
  labels: string[];
  allowed_classification_max: "public" | "internal" | "confidential" | "restricted" | "secret";
  allow_code_context: boolean;
  allow_project_memory: boolean;
  allow_conversation_history: boolean;
  require_redaction: boolean;
  external_processing_allowed: boolean;
  retention_policy: "none" | "session" | "provider_default";
  created_at: string;
  updated_at: string;
}

export interface CreateAgentProfileInput {
  name: string;
  provider: string;
  model: string;
  base_url?: string;
  role_prompt?: string;
  api_key: string;
  enabled?: boolean;
  labels?: string[];
  allowed_classification_max?: string;
  allow_code_context?: boolean;
  allow_project_memory?: boolean;
  allow_conversation_history?: boolean;
  require_redaction?: boolean;
  external_processing_allowed?: boolean;
  retention_policy?: string;
}

export interface UpdateAgentProfileInput {
  name?: string;
  provider?: string;
  model?: string;
  base_url?: string;
  role_prompt?: string;
  api_key?: string;
  labels?: string[];
  enabled?: boolean;
  allowed_classification_max?: string;
  allow_code_context?: boolean;
  allow_project_memory?: boolean;
  allow_conversation_history?: boolean;
  require_redaction?: boolean;
  external_processing_allowed?: boolean;
  retention_policy?: string;
}

export interface Conversation {
  id: string;
  project_id: string;
  user_id: string;
  title: string;
  mode: ChatMode;
  created_at: string;
  updated_at: string;
}

export interface Message {
  id: string;
  conversation_id: string;
  role: "user" | "hermes" | "openclaw" | "system";
  content: string;
  agent_name?: string;
  file_path?: string;
  created_at: string;
  /**
   * Author identity for `role: "user"` messages. Populated from migration
   * 0021 onward; older rows backfilled to the conversation creator. Null
   * on assistant / system rows.
   */
  user_id?: string | null;
  /**
   * Display name JOINed from `users` on read paths. May be absent on the
   * immediate INSERT response — UI falls back to "You" when missing.
   */
  author_name?: string | null;
}

export interface ConversationWithMessages extends Conversation {
  messages: Message[];
}

export interface ProjectMemoryCandidate {
  id: string;
  project_id: string;
  candidate_type: "project_summary";
  proposed_content: string;
  source_message_count: number;
  source_context_hash: string;
  status: "pending" | "approved" | "rejected";
  review_note?: string | null;
  reviewed_by?: string | null;
  created_at: string;
  reviewed_at?: string | null;
  applied_at?: string | null;
}

export type TaskStatus = "todo" | "in-progress" | "done" | "cancelled";
export type TaskPriority = "low" | "medium" | "high" | "critical";

export interface AcceptanceCriteriaV2 {
  tests?: string[];
  commands?: string[];
  diff_hints?: string[];
  behavior?: string[];
}

export interface ProjectTask {
  id: string;
  project_id: string;
  title: string;
  why?: string | null;
  affected_files?: string[] | null;
  acceptance_criteria?: string | null;
  estimated_effort?: string | null;
  priority: TaskPriority;
  status: TaskStatus;
  source_message_id?: string | null;
  source_conversation_id?: string | null;
  // P1 fields
  assignee?: string | null;
  due_date?: string | null;             // ISO date "YYYY-MM-DD"
  test_plan?: string | null;
  rollback_plan?: string | null;
  definition_of_done?: string | null;
  labels: string[];                     // always present, possibly empty
  // P2 fields
  acceptance_criteria_v2?: AcceptanceCriteriaV2 | null;
  linked_pr_url?: string | null;
  linked_commit_sha?: string | null;
  depends_on: string[];                 // always present, possibly empty
  // P3 sprint binding
  sprint_id?: string | null;
  sprint_name?: string | null;
  /** Server-computed via task_comments JOIN. Updated when re-listing. */
  comment_count?: number;
  // B3 epic binding (cross-project)
  epic_id?: string | null;
  epic_name?: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateTaskInput {
  title: string;
  why?: string;
  affected_files?: string[];
  acceptance_criteria?: string;
  estimated_effort?: string;
  priority?: TaskPriority;
  source_message_id?: string;
  assignee?: string;
  due_date?: string;
  test_plan?: string;
  rollback_plan?: string;
  definition_of_done?: string;
  labels?: string[];
  acceptance_criteria_v2?: AcceptanceCriteriaV2 | null;
  linked_pr_url?: string;
  linked_commit_sha?: string;
  depends_on?: string[];
  sprint_id?: string;
  epic_id?: string;
}

export interface UpdateTaskInput {
  title?: string;
  why?: string;
  affected_files?: string[];
  acceptance_criteria?: string;
  estimated_effort?: string;
  priority?: TaskPriority;
  status?: TaskStatus;
  assignee?: string;
  due_date?: string | null;
  test_plan?: string;
  rollback_plan?: string;
  definition_of_done?: string;
  labels?: string[];
  acceptance_criteria_v2?: AcceptanceCriteriaV2 | null;
  linked_pr_url?: string;
  linked_commit_sha?: string;
  depends_on?: string[];
  /** P3: send a UUID string to bind to a sprint, send `null` to clear,
   *  omit to leave alone. */
  sprint_id?: string | null;
  /** B3: same semantics as sprint_id, for cross-project epic binding. */
  epic_id?: string | null;
  status_note?: string;
}

export interface TaskAttempt {
  id: string;
  task_id: string;
  conversation_id: string;
  mode: string;
  status: "pending" | "running" | "complete" | "failed" | "cancelled";
  dispatched_by?: string | null;
  dispatched_by_name?: string | null;
  note?: string | null;
  created_at: string;
  updated_at: string;
}

export interface DispatchTaskInput {
  mode: string;
  note?: string;
  conversation_id?: string;
  title?: string;
}

export interface DispatchTaskResult {
  attempt: TaskAttempt;
  conversation_id: string;
  prompt: string;
}

export interface Sprint {
  id: string;
  project_id: string;
  name: string;
  goal?: string | null;
  start_date?: string | null;
  end_date?: string | null;
  status: "planned" | "active" | "closed";
  task_total: number;
  task_done: number;
  created_at: string;
  updated_at: string;
}

export interface CreateSprintInput {
  name: string;
  goal?: string;
  start_date?: string;
  end_date?: string;
  status?: "planned" | "active" | "closed";
}

export interface UpdateSprintInput {
  name?: string;
  goal?: string;
  start_date?: string;
  end_date?: string;
  status?: "planned" | "active" | "closed";
}

// ---------------------------------------------------------------
// B3 Epics — user-scoped cross-project milestone buckets
// ---------------------------------------------------------------
export interface Epic {
  id: string;
  user_id: string;
  name: string;
  description?: string | null;
  color?: string | null;
  status: "planned" | "active" | "done" | "archived";
  target_date?: string | null;
  task_total: number;
  task_done: number;
  project_count: number;
  created_at: string;
  updated_at: string;
}

export interface CreateEpicInput {
  name: string;
  description?: string;
  color?: string;
  status?: Epic["status"];
  target_date?: string;
}
export interface UpdateEpicInput {
  name?: string;
  description?: string;
  color?: string;
  status?: Epic["status"];
  target_date?: string;
}

export const epics = {
  list: () => request<Epic[]>("/epics"),
  create: (data: CreateEpicInput) =>
    request<Epic>("/epics", { method: "POST", body: JSON.stringify(data) }),
  update: (id: string, data: UpdateEpicInput) =>
    request<Epic>(`/epics/${id}`, { method: "PATCH", body: JSON.stringify(data) }),
  delete: (id: string) => request<void>(`/epics/${id}`, { method: "DELETE" }),
};

// ---------------------------------------------------------------
// B6 Shared memory notes
// ---------------------------------------------------------------
export interface SharedMemoryNote {
  id: string;
  user_id: string;
  title: string;
  body: string;
  tags: string[];
  scope_projects: string[];
  pinned: boolean;
  created_at: string;
  updated_at: string;
}
export interface CreateNoteInput {
  title: string;
  body: string;
  tags?: string[];
  scope_projects?: string[];
  pinned?: boolean;
}
export interface UpdateNoteInput {
  title?: string;
  body?: string;
  tags?: string[];
  scope_projects?: string[];
  pinned?: boolean;
}
export const sharedMemory = {
  list: (opts?: { projectId?: string; q?: string }) => {
    const params = new URLSearchParams();
    if (opts?.projectId) params.set("project_id", opts.projectId);
    if (opts?.q) params.set("q", opts.q);
    const qs = params.toString();
    return request<SharedMemoryNote[]>(`/memory${qs ? `?${qs}` : ""}`);
  },
  create: (data: CreateNoteInput) =>
    request<SharedMemoryNote>("/memory", { method: "POST", body: JSON.stringify(data) }),
  update: (id: string, data: UpdateNoteInput) =>
    request<SharedMemoryNote>(`/memory/${id}`, { method: "PATCH", body: JSON.stringify(data) }),
  delete: (id: string) => request<void>(`/memory/${id}`, { method: "DELETE" }),
};

// ---------------------------------------------------------------
// User-level cross-project views (B1, B2, B4, B5)
// ---------------------------------------------------------------
export interface UserTask {
  id: string;
  project_id: string;
  project_name: string;
  title: string;
  status: TaskStatus;
  priority: TaskPriority;
  assignee?: string | null;
  due_date?: string | null;
  labels: string[];
  sprint_id?: string | null;
  sprint_name?: string | null;
  epic_id?: string | null;
  epic_name?: string | null;
  linked_pr_url?: string | null;
  comment_count: number;
  updated_at: string;
}
export interface ProjectUsage {
  project_id: string;
  project_name: string;
  /** mig 0023: true when the underlying project was deleted but its
   *  historical cost events are still attributed to this user via the
   *  snapshot column. Frontend renders these rows with a muted /
   *  strikethrough label. */
  project_deleted?: boolean;
  calls: number;
  tokens_in: number;
  tokens_out: number;
  cost_usd: number;
}
export interface AgentUsage {
  agent: string;
  calls: number;
  tokens_in: number;
  tokens_out: number;
  cost_usd: number;
}
export interface UserUsage {
  by_project: ProjectUsage[];
  by_agent: AgentUsage[];
  total_calls: number;
  total_tokens_in: number;
  total_tokens_out: number;
  total_cost_usd: number;
  daily: Array<{ day: string; calls: number; cost_usd: number }>;
  days: number;
}
export interface ProjectDebateHealth {
  project_id: string;
  project_name: string;
  /** Same semantics as ProjectUsage.project_deleted — true when the
   *  source project has been deleted but events still attribute to
   *  this user. mig 0023. */
  project_deleted?: boolean;
  debate_turns: number;
  consensus_turns: number;
  citation_turns: number;
}
export interface RoundBucket {
  rounds: number;
  count: number;
}
export interface DebateHealth {
  days: number;
  total_debate_turns: number;
  consensus_rate: number;
  file_citation_rate: number;
  by_project: ProjectDebateHealth[];
  round_distribution: RoundBucket[];
}
export interface ConvHit {
  conversation_id: string;
  project_id: string;
  project_name: string;
  title: string;
  mode: string;
  message_id?: string | null;
  snippet?: string | null;
  updated_at: string;
}
export interface FileHit {
  project_id: string;
  project_name: string;
  path: string;
  size_bytes?: number | null;
}
export const userViews = {
  tasks: (opts?: { projectId?: string; epicId?: string; status?: string; assignee?: string; label?: string; q?: string }) => {
    const params = new URLSearchParams();
    if (opts?.projectId) params.set("project_id", opts.projectId);
    if (opts?.epicId) params.set("epic_id", opts.epicId);
    if (opts?.status) params.set("status", opts.status);
    if (opts?.assignee) params.set("assignee", opts.assignee);
    if (opts?.label) params.set("label", opts.label);
    if (opts?.q) params.set("q", opts.q);
    const qs = params.toString();
    return request<UserTask[]>(`/user/tasks${qs ? `?${qs}` : ""}`);
  },
  usage: (days?: number) =>
    request<UserUsage>(`/user/usage${days ? `?days=${days}` : ""}`),
  debateHealth: (days?: number) =>
    request<DebateHealth>(`/user/debate-health${days ? `?days=${days}` : ""}`),
  conversations: (q: string, limit?: number) => {
    const params = new URLSearchParams({ q });
    if (limit) params.set("limit", String(limit));
    return request<ConvHit[]>(`/user/conversations?${params}`);
  },
  code: (q: string, limit?: number) => {
    const params = new URLSearchParams({ q });
    if (limit) params.set("limit", String(limit));
    return request<FileHit[]>(`/user/code?${params}`);
  },
};

export const sprints = {
  list: (projectId: string) =>
    request<Sprint[]>(`/projects/${projectId}/sprints`),
  create: (projectId: string, data: CreateSprintInput) =>
    request<Sprint>(`/projects/${projectId}/sprints`, {
      method: "POST",
      body: JSON.stringify(data),
    }),
  update: (projectId: string, sprintId: string, data: UpdateSprintInput) =>
    request<Sprint>(`/projects/${projectId}/sprints/${sprintId}`, {
      method: "PATCH",
      body: JSON.stringify(data),
    }),
  delete: (projectId: string, sprintId: string) =>
    request<void>(`/projects/${projectId}/sprints/${sprintId}`, { method: "DELETE" }),
};

export interface TaskComment {
  id: string;
  task_id: string;
  user_id: string;
  author_name?: string | null;
  content: string;
  created_at: string;
  updated_at: string;
}

export interface TaskStatusEvent {
  id: string;
  task_id: string;
  from_status: TaskStatus | null;
  to_status: TaskStatus;
  changed_by?: string | null;
  changed_by_name?: string | null;
  note?: string | null;
  changed_at: string;
}

export interface MetricsHealth {
  score: number;
  confidence?: number;
  methodology?: string;
  limitations?: string[];
  indexed_files: number;
  signals?: Record<string, number>;
  dimensions: Array<{
    key: string;
    label: string;
    score: number;
    level: "Low" | "Medium" | "High";
    confidence?: number;
    measured_by?: string;
    formula?: string;
    evidence: string;
    evidence_items?: string[];
  }>;
}

export interface MetricsBurndownPoint {
  day: string;       // YYYY-MM-DD
  total: number;     // cumulative tasks created by EOD
  done: number;      // cumulative tasks completed by EOD
  remaining: number; // total - done
  ideal: number;     // linear reference trajectory
}

export interface MetricsBurndown {
  points: MetricsBurndownPoint[];
  final_total: number;
  final_remaining: number;
  velocity_per_day: number;
}

export interface MetricsCost {
  by_agent: Array<{ agent: string; tokens_in: number; tokens_out: number; calls: number; cost_usd: number }>;
  by_mode: Array<{ mode: string; tokens_in: number; tokens_out: number; calls: number; cost_usd: number }>;
  daily: Array<{ day: string; agent: string; tokens_in: number; tokens_out: number; cost_usd: number }>;
  total_cost_usd: number;
  pricing: {
    openclaw_per_1k_in: number;
    openclaw_per_1k_out: number;
    hermes_per_1k_in: number;
    hermes_per_1k_out: number;
  };
  note: string;
}

export interface MetricsSummary {
  totals: {
    conversations: number;
    messages: number;
    user_messages: number;
    agent_messages: number;
  };
  mode_distribution: Array<{ mode: string; count: number }>;
  avg_chars_by_agent: Array<{ agent: string; avg_chars: number; response_count: number }>;
  consensus: { debate_finals: number; with_consensus: number; rate: number };
  debate_round_distribution: Array<{ round: number; count: number }>;
  timing: {
    avg_ttft_ms: number | null;
    avg_total_ms: number | null;
    p50_total_ms: number | null;
    p95_total_ms: number | null;
  };
  file_citation: { total: number; with_citation: number; rate: number };
  feedback_by_agent: Array<{
    agent: string;
    thumbs_up: number;
    thumbs_down: number;
    total: number;
    satisfaction_rate: number;
  }>;
}
