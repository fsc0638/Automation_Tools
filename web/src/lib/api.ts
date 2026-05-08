const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:8080/api";

function getToken(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem("kway_token");
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = getToken();
  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...options.headers,
    },
  });

  // Auto-logout on expired/invalid session: when we sent a token but the
  // server rejected it, clear local state and redirect to /login.
  // Skipped for login/register pages so a wrong-password 401 stays as
  // an inline form error rather than a redirect loop.
  if (res.status === 401 && token && typeof window !== "undefined") {
    const currentPath = window.location.pathname;
    if (currentPath !== "/login" && currentPath !== "/register") {
      localStorage.removeItem("kway_token");
      window.location.replace("/login");
      // Block this promise so callers don't surface a runtime error during
      // the brief moment before the navigation actually happens.
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
    request<{ access_token: string; user: UserInfo }>("/auth/register", {
      method: "POST",
      body: JSON.stringify(data),
    }),
  login: (data: { email: string; password: string }) =>
    request<{ access_token: string; user: UserInfo }>("/auth/login", {
      method: "POST",
      body: JSON.stringify(data),
    }),
};

// Projects
export const projects = {
  list: () => request<Project[]>("/projects"),
  create: (data: CreateProjectInput) =>
    request<Project>("/projects", { method: "POST", body: JSON.stringify(data) }),
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
  metricsHealth: (id: string) =>
    request<MetricsHealth>(`/projects/${id}/metrics/health`),
  remoteBranches: (url: string, git_identity_id?: string) =>
    request<{ branches: string[] }>("/git/remote-branches", {
      method: "POST",
      body: JSON.stringify({ url, git_identity_id: git_identity_id ?? null }),
    }),
};

export const tasks = {
  list: (projectId: string) =>
    request<ProjectTask[]>(`/projects/${projectId}/tasks`),
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
};

export const gitIdentities = {
  list: () => request<GitIdentity[]>("/git/identities"),
  create: (data: { name: string; provider?: string; username: string; access_token: string; repository_url?: string }) =>
    request<GitIdentity>("/git/identities", { method: "POST", body: JSON.stringify(data) }),
  delete: (id: string) => request<void>(`/git/identities/${id}`, { method: "DELETE" }),
};

// Conversations
export const conversations = {
  list: (projectId: string, mode?: AgentMode) =>
    request<Conversation[]>(`/projects/${projectId}/conversations${mode ? `?mode=${mode}` : ""}`),
  create: (projectId: string, title?: string, mode?: AgentMode) =>
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
};

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

export interface CreateProjectInput {
  name: string;
  description?: string;
  source_type: string;
  source_path: string;
  git_identity_id?: string;
  default_branch?: string;
}

export interface Project {
  id: string;
  user_id: string;
  name: string;
  description?: string;
  source_type: string;
  source_path: string;
  local_path?: string;
  default_branch?: string;
  git_identity_id?: string;
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

export interface Conversation {
  id: string;
  project_id: string;
  user_id: string;
  title: string;
  mode: AgentMode;
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
}

export interface ConversationWithMessages extends Conversation {
  messages: Message[];
}

export type TaskStatus = "todo" | "in-progress" | "done" | "cancelled";
export type TaskPriority = "low" | "medium" | "high" | "critical";

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
}

export interface UpdateTaskInput {
  title?: string;
  why?: string;
  affected_files?: string[];
  acceptance_criteria?: string;
  estimated_effort?: string;
  priority?: TaskPriority;
  status?: TaskStatus;
}

export interface MetricsHealth {
  score: number;
  indexed_files: number;
  dimensions: Array<{
    key: string;
    label: string;
    score: number;
    level: "Low" | "Medium" | "High";
    evidence: string;
  }>;
}

export interface MetricsCost {
  by_agent: Array<{ agent: string; tokens_out: number; calls: number; cost_usd: number }>;
  by_mode: Array<{ mode: string; tokens_out: number; calls: number; cost_usd: number }>;
  daily: Array<{ day: string; agent: string; tokens_out: number; cost_usd: number }>;
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
}
