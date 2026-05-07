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
  get: (id: string) => request<Project>(`/projects/${id}`),
  delete: (id: string) => request<void>(`/projects/${id}`, { method: "DELETE" }),
  fileTree: (id: string) => request<FileNode[]>(`/projects/${id}/files`),
  fileContent: (id: string, path: string) =>
    request<{ path: string; content: string }>(`/projects/${id}/files/content?path=${encodeURIComponent(path)}`),
  gitStatus: (id: string) => request<GitStatus>(`/projects/${id}/git/status`),
  gitBranches: (id: string) => request<{ branches: string[] }>(`/projects/${id}/git/branches`),
  checkoutBranch: (id: string, branch: string) =>
    request<Project>(`/projects/${id}/git/checkout`, { method: "POST", body: JSON.stringify({ branch }) }),
  remoteBranches: (url: string, git_identity_id?: string) =>
    request<{ branches: string[] }>("/git/remote-branches", {
      method: "POST",
      body: JSON.stringify({ url, git_identity_id: git_identity_id ?? null }),
    }),
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
