"use client";
import { memo, use, useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import ReactMarkdown from "react-markdown";
import { SyntaxHighlighter } from "@/components/SyntaxHighlighter";
import {
  Activity,
  AlertCircle,
  ArrowDown,
  ArrowLeft,
  Bot,
  ChevronDown,
  ChevronRight,
  ClipboardList,
  Clock3,
  Code2,
  Cpu,
  DollarSign,
  File,
  FolderOpen,
  GitBranch,
  Lightbulb,
  ListChecks,
  Map as MapIcon,
  MessageSquarePlus,
  PieChart,
  Plus,
  RefreshCw,
  Search,
  Send,
  Sparkles,
  Square,
  ThumbsDown,
  ThumbsUp,
  Trash2,
  User,
  Zap,
} from "lucide-react";
import {
  conversations as convsApi,
  createWsConnection,
  agentProfiles as agentProfilesApi,
  feedback as feedbackApi,
  projects as projectsApi,
  tasks as tasksApi,
  type AgentProfile,
  type ChatMode,
  type AgentMode,
  type Conversation,
  type ConversationSummary,
  type FileNode,
  type GitStatus,
  type Message,
  type Project,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { InlineBanner, SectionEmpty, SkeletonBlock } from "@/components/ui/card";
import { cn, formatDate } from "@/lib/utils";
import { useToastStore } from "@/lib/toast-store";
import { useT } from "@/lib/i18n";
import { useAuthStore, useWorkspaceChromeStore } from "@/lib/store";
import { InsightsTab } from "@/components/InsightsTab";
import { CostTab } from "@/components/CostTab";
import { RoadmapTab } from "@/components/RoadmapTab";

type ProjectTab = "workspace" | "insights" | "cost" | "roadmap";

type QuickAction = "health" | "explore" | "roadmap" | "patch";

const QUICK_ACTIONS: Array<{ key: QuickAction; labelKey: string; icon: typeof Activity; prompt: string }> = [
  {
    key: "health",
    labelKey: "quick.healthScan",
    icon: Activity,
    prompt:
      "請以 Debate Mode 執行專案初診。OpenClaw 從架構、系統風險、資料流與長期維護角度分析；Hermes 從實作成本、可讀性、日常維護、測試與快速改善角度分析。請根據已索引的專案檔案提出：1. 專案摘要 2. 技術棧與入口點 3. 主要風險 4. 可立即改善項目 5. 中長期優化方向 6. 測試/文件缺口。所有具體判斷都要引用檔案路徑作為依據；如果證據不足，明確說明。最後產生優先順序清楚的結論。",
  },
  {
    key: "explore",
    labelKey: "quick.exploreIdeas",
    icon: Lightbulb,
    prompt:
      "請進入問題探索模式。不要只回答單一問題，請讓 OpenClaw / Hermes 主動碰撞這個專案可能值得改善、重構或產品化的方向。輸出：潛在問題、可驗證假設、使用者可能真正想解決的需求、創新功能想法、風險與取捨。每個建議都要盡可能引用已索引檔案路徑，並標示信心等級與下一步驗證方式。",
  },
  {
    key: "roadmap",
    labelKey: "quick.roadmap",
    icon: ListChecks,
    prompt:
      "請把目前專案可優化方向整理成可執行 Roadmap，必須根據專案檔案與目前對話，不要憑空發明。\n\n輸出格式：先用一段 markdown 敘述為什麼這些任務重要，接著一個 ```json fenced block，內容是 JSON array，每個元素一個任務：\n\n```json\n[\n  {\n    \"title\": \"短句任務名稱（≤80 字）\",\n    \"priority\": \"low | medium | high | critical\",\n    \"why\": \"為什麼這項重要、會解決什麼問題\",\n    \"affected_files\": [\"path/to/foo.ts\", \"backend/src/bar.rs\"],\n    \"acceptance_criteria\": \"開發者或代理人可驗證的條件：例如『跑 X 測試會通過』、『B 檔案的 Y 函式變成 Z 行為』、『diff 不超過 N 行』\",\n    \"estimated_effort\": \"S | M | L 或 0.5d / 2d 等\"\n  }\n]\n```\n\n至少 3 項、最多 8 項。每個任務都必須引用真實的檔案路徑或對話中提到的問題。",
  },
  {
    key: "patch",
    labelKey: "quick.patchPlan",
    icon: Code2,
    prompt:
      "請進入 Patch / PR 規劃模式。根據目前專案狀態，挑選最高價值且風險可控的 1~3 項改善，產生 patch-ready 計畫。\n\n輸出格式：先用 markdown 說明選擇這些 patch 的理由，再用 ```json fenced block 給出陣列，每個 patch 一個物件：\n\n```json\n[\n  {\n    \"title\": \"PR 標題（祈使句、≤72 字）\",\n    \"priority\": \"low | medium | high | critical\",\n    \"why\": \"目標、影響範圍、預期收益\",\n    \"affected_files\": [\"路徑1\", \"路徑2\"],\n    \"acceptance_criteria\": \"明確驗證條件，包含：1) 應通過的測試指令 2) 預期 diff 摘要 3) 行為驗證步驟 4) 回滾方案\",\n    \"estimated_effort\": \"S | M | L\"\n  }\n]\n```\n\n不要實際 commit 或 push；若證據不足，先列出需要讀取或確認的檔案，並說明該 patch 為何尚不該實作。",
  },
];


const MODE_LABELS: Record<AgentMode, string> = {
  openclaw: "OpenClaw",
  hermes: "Hermes",
  debate: "Debate Mode",
};

const MODE_STYLES: Record<AgentMode, string> = {
  openclaw: "bg-blue-50 text-[#0050A0] border border-[#BFDBFE]",
  hermes: "bg-violet-50 text-[#7C3AED] border border-[#DDD6FE]",
  debate: "bg-amber-50 text-[#B45309] border border-[#FDE68A]",
};

function isCoreAgentMode(value: ChatMode): value is AgentMode {
  return value === "openclaw" || value === "hermes" || value === "debate";
}

function modeLabel(value: ChatMode, profiles: AgentProfile[] = [], t?: (key: string) => string) {
  if (isCoreAgentMode(value)) {
    if (t) {
      if (value === "openclaw") return t("chat.modeOpenClaw");
      if (value === "hermes") return t("chat.modeHermes");
      return t("chat.modeDebate");
    }
    return MODE_LABELS[value];
  }
  if (value.startsWith("agents:")) return t ? t("chat.modeCustomDebate") : "Custom Debate";
  const id = value.startsWith("agent:") ? value.slice("agent:".length) : "";
  return profiles.find((profile) => profile.id === id)?.name ?? (t ? t("chat.modeCustomAgent") : "Custom Agent");
}

function modeStyle(value: ChatMode) {
  if (isCoreAgentMode(value)) return MODE_STYLES[value];
  if (value.startsWith("agents:")) return "bg-teal-50 text-teal-700 border border-teal-200";
  return "bg-emerald-50 text-emerald-700 border border-emerald-200";
}

/**
 * Infer the conversation's display mode/label from its title.
 *
 * Reason: `conversations.mode` in the DB is restricted by a CHECK
 * constraint (mig 0002) to "openclaw" | "hermes" | "debate", so a
 * custom-agent or custom-debate conversation always lands as
 * "openclaw". The auto-generated title carries the real intent:
 *   - "Custom Debate Conversation N" → custom debate
 *   - "<AgentName> Conversation N"   → that single custom agent
 *   - "OpenClaw Conversation N"      → just OpenClaw (matches conv.mode)
 *   - "Hermes / Debate Mode …"       → matches conv.mode
 * Until backlog #25 widens the CHECK constraint and we can store
 * the real mode string, we recover the intent by matching the
 * title prefix against the enabled agent profile list.
 *
 * Returns null when the title carries no usable hint and the caller
 * should fall back to the raw `conv.mode` styling.
 */
function inferConversationMode(
  conv: { title: string; mode: ChatMode },
  profiles: AgentProfile[] = [],
):
  | { label: string; className: string }
  | null {
  const title = conv.title.trim();
  if (title.startsWith("Custom Debate")) {
    return {
      label: "Custom Debate",
      className: "bg-teal-50 text-teal-700 border border-teal-200",
    };
  }
  // Match any enabled custom agent profile by name prefix. Sort by
  // length descending so a longer name ("Gemini-lite") wins over a
  // shorter one ("Gemini") that would otherwise match first.
  const sorted = [...profiles].sort((a, b) => b.name.length - a.name.length);
  for (const profile of sorted) {
    if (title.startsWith(`${profile.name} `) || title === profile.name) {
      return {
        label: profile.name,
        className: "bg-emerald-50 text-emerald-700 border border-emerald-200",
      };
    }
  }
  return null;
}

type StreamStatus = {
  agent: string;
  message: string;
  round?: number;
  phase?: string;
  startedAt: number;
};

type ContextTab = "files" | "git" | "project";

function displayAgentName(agent: string, round?: number, phase?: string) {
  if (phase === "round" && round) return `${agent} · Round ${round}`;
  if (phase === "final") return `${agent} · Final`;
  return agent;
}

function detectLanguage(path: string) {
  const ext = path.split(".").pop()?.toLowerCase();
  switch (ext) {
    case "rs": return "rust";
    case "ts":
    case "tsx": return "typescript";
    case "js":
    case "jsx": return "javascript";
    case "py": return "python";
    case "json": return "json";
    case "md": return "markdown";
    case "css": return "css";
    case "html": return "html";
    case "sql": return "sql";
    case "swift": return "swift";
    case "yml":
    case "yaml": return "yaml";
    default: return "text";
  }
}

function flattenFilePaths(nodes: FileNode[]): string[] {
  const results: string[] = [];
  for (const node of nodes) {
    if (node.is_dir) {
      if (node.children?.length) results.push(...flattenFilePaths(node.children));
    } else {
      results.push(node.path);
    }
  }
  return results;
}

function formatRelativeTime(value: string) {
  const date = new Date(value).getTime();
  if (Number.isNaN(date)) return "Unknown";
  const diffMs = Date.now() - date;
  const minutes = Math.max(1, Math.floor(diffMs / 60000));
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return formatDate(value);
}

export default function ProjectPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = use(params);
  const router = useRouter();
  // Deep-link target tab. The global Roadmap (/roadmap) links each card
  // to /projects/{id}?tab=roadmap so the user lands on that project's
  // Roadmap board, not the chat composer. Insights / Cost can be reached
  // the same way later if other surfaces want to deep-link them.
  const searchParams = useSearchParams();
  const pushToast = useToastStore((state) => state.pushToast);
  const t = useT();
  const setShowAppSidebar = useWorkspaceChromeStore((state) => state.setShowAppSidebar);
  // Identity of the logged-in viewer. Used below to decide whether the
  // conversation-row delete affordance should render: editors should only
  // see it on conversations they authored. Project owners (matched by
  // project.user_id) keep the affordance on every row.
  const currentUserId = useAuthStore((state) => state.user?.id);

  const [project, setProject] = useState<Project | null>(null);
  const [fileTree, setFileTree] = useState<FileNode[]>([]);
  const [branches, setBranches] = useState<string[]>([]);
  const [gitStatus, setGitStatus] = useState<GitStatus | null>(null);
  const [switchingBranch, setSwitchingBranch] = useState(false);
  const [convs, setConvs] = useState<Conversation[]>([]);
  const [agentProfiles, setAgentProfiles] = useState<AgentProfile[]>([]);
  const [showCustomDebatePicker, setShowCustomDebatePicker] = useState(false);
  // "New conversation" type chooser. Opens when the user clicks any of the
  // four `+ New Conversation` buttons in the chat shell. The previous flow
  // just created a conversation with the currently-selected composer mode,
  // which hid the agent picker from anyone who didn't already understand
  // the mode toggle row.
  const [showNewConvModal, setShowNewConvModal] = useState(false);
  // When the user picks "Custom Debate" inside the new-conversation modal
  // we chain through to the existing CustomDebatePicker. This flag tells the
  // picker that on confirm it should CREATE the conversation (not just
  // update the active composer's mode).
  const [pickerCreatesConv, setPickerCreatesConv] = useState(false);
  const [activeConv, setActiveConv] = useState<Conversation | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<ChatMode>("openclaw");
  const [streaming, setStreaming] = useState(false);
  const [streamBuffers, setStreamBuffers] = useState<Record<string, string>>({});
  const [streamStatuses, setStreamStatuses] = useState<Record<string, StreamStatus>>({});
  const [statusNow, setStatusNow] = useState(0);
  const [mountedAt] = useState(() => Date.now());
  const [showJumpToBottom, setShowJumpToBottom] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshStatus, setRefreshStatus] = useState("");
  const [conversationQuery, setConversationQuery] = useState("");
  const [contextTab, setContextTab] = useState<ContextTab>("files");
  const [projectTab, setProjectTab] = useState<ProjectTab>(() => {
    // Read the desired tab from the URL once on mount. After this, manual
    // tab clicks update local state only — we don't rewrite the URL on
    // every tab switch to avoid spurious browser-history entries.
    const requested = searchParams.get("tab");
    if (requested === "roadmap" || requested === "insights" || requested === "cost") {
      return requested;
    }
    return "workspace";
  });
  const [fileQuery, setFileQuery] = useState("");
  const [selectedFilePath, setSelectedFilePath] = useState("");
  const [selectedFileContent, setSelectedFileContent] = useState("");
  const [filePreviewLoading, setFilePreviewLoading] = useState(false);
  const [filePreviewError, setFilePreviewError] = useState("");
  const [showConversationRail, setShowConversationRail] = useState(true);
  const [showContextRail, setShowContextRail] = useState(false);
  const [showWorkspaceOverview, setShowWorkspaceOverview] = useState(false);
  const [showConversationSummary, setShowConversationSummary] = useState(false);
  // Cached per-conversation summary refreshed server-side after each turn.
  // Null while loading / before the first turn has produced one.
  const [convSummary, setConvSummary] = useState<ConversationSummary | null>(null);
  const [showDebateWorkflow, setShowDebateWorkflow] = useState(false);
  const [showComposerTools, setShowComposerTools] = useState(false);
  const [focusMode, setFocusMode] = useState(false);

  const streamBuffersRef = useRef<Record<string, string>>({});
  const streamStatusesRef = useRef<Record<string, StreamStatus>>({});
  const wsRef = useRef<WebSocket | null>(null);
  const [wsReconnectKey, setWsReconnectKey] = useState(0);

  /**
   * Close a WebSocket safely under React 18+ StrictMode double-mount. When
   * the effect cleanup fires while the socket is still CONNECTING, calling
   * `close()` produces the "WebSocket closed before the connection is
   * established" console warning. Defer the close to the `open` event so
   * the handshake completes first, then close cleanly.
   */
  function safeCloseWs(ws: WebSocket | null) {
    if (!ws) return;
    if (ws.readyState === WebSocket.CONNECTING) {
      ws.addEventListener("open", () => ws.close(), { once: true });
      return;
    }
    if (ws.readyState === WebSocket.OPEN) {
      ws.close();
    }
  }
  const messagesScrollRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const shouldAutoScrollRef = useRef(true);
  const scrollRafRef = useRef<number | null>(null);
  const flushRafRef = useRef<number | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const filteredConvs = useMemo(() => {
    const query = conversationQuery.trim().toLowerCase();
    if (!query) return convs;
    return convs.filter((conv) => (
      `${conv.title} ${MODE_LABELS[conv.mode]}`.toLowerCase().includes(query)
    ));
  }, [convs, conversationQuery]);

  const dirtyCount = (gitStatus?.changed.length ?? 0) + (gitStatus?.staged.length ?? 0) + (gitStatus?.untracked.length ?? 0);
  const focusedFileName = selectedFilePath ? selectedFilePath.split("/").pop() ?? selectedFilePath : "";
  const allFilePaths = useMemo(() => flattenFilePaths(fileTree), [fileTree]);
  const fileHits = useMemo(() => {
    const query = fileQuery.trim().toLowerCase();
    if (!query) return allFilePaths.slice(0, 8);
    return allFilePaths.filter((path) => path.toLowerCase().includes(query)).slice(0, 10);
  }, [allFilePaths, fileQuery]);
  const gitHotspots = useMemo(() => {
    const unique = Array.from(new Set([...(gitStatus?.changed ?? []), ...(gitStatus?.staged ?? []), ...(gitStatus?.untracked ?? [])]));
    return unique.slice(0, 6);
  }, [gitStatus]);
  const workspaceInsights = useMemo(() => {
    const items: { title: string; detail: string; action: string }[] = [];
    if (dirtyCount > 0) {
      items.push({
        title: "Review pending changes",
        detail: `${dirtyCount} file(s) differ from the current branch baseline.`,
        action: "Summarize change risk and next steps before editing more code.",
      });
    }
    if (selectedFilePath) {
      items.push({
        title: "Focused file context available",
        detail: focusedFileName,
        action: `Analyze file: ${selectedFilePath}. Explain purpose, risk, and likely edits.`,
      });
    }
    if ((convs.length ?? 0) > 0) {
      items.push({
        title: "Shared project memory is active",
        detail: `${convs.length} conversation(s) contribute to project context.`,
        action: "Summarize what the team has learned so far and identify the best next implementation step.",
      });
    }
    if (project?.source_type === "git") {
      items.push({
        title: "Repository-aware workspace",
        detail: `Current branch: ${project.default_branch ?? "unknown"}`,
        action: `Review the branch state for ${project.default_branch ?? "this workspace"} and list the safest next action.`,
      });
    }
    return items.slice(0, 3);
  }, [convs.length, dirtyCount, focusedFileName, project, selectedFilePath]);
  const activeThreadSummary = useMemo(() => {
    const nonSystem = messages.filter((msg) => msg.role !== "system");
    const lastAgent = [...nonSystem].reverse().find((msg) => msg.role === "hermes" || msg.role === "openclaw");
    const lastUser = [...nonSystem].reverse().find((msg) => msg.role === "user");
    return {
      messageCount: nonSystem.length,
      lastAgent: lastAgent?.agent_name ?? (lastAgent?.role ? MODE_LABELS[lastAgent.role as Extract<AgentMode, "hermes" | "openclaw">] : null),
      lastUserAt: lastUser?.created_at ?? null,
    };
  }, [messages]);
  const conversationHealth = useMemo(() => {
    const recentCount = convs.filter((conv) => mountedAt - new Date(conv.updated_at).getTime() < 1000 * 60 * 60 * 24).length;
    const debateCount = convs.filter((conv) => conv.mode === "debate").length;
    return {
      total: convs.length,
      recent: recentCount,
      debate: debateCount,
    };
  }, [convs, mountedAt]);
  const streamStatusEntries = useMemo(() => Object.entries(streamStatuses), [streamStatuses]);
  const streamBufferEntries = useMemo(() => Object.entries(streamBuffers), [streamBuffers]);
  const debateTimeline = useMemo(() => {
    const statusText = streamStatusEntries.map(([, status]) => `${status.agent} ${status.phase ?? ""} ${status.message}`.toLowerCase()).join(" ");
    const hasOpenClaw = statusText.includes("openclaw");
    const hasHermes = statusText.includes("hermes");
    const hasFinal = statusText.includes("final") || statusText.includes("synthesis");
    return [
      {
        title: t("debate.stepOpenClaw"),
        detail: t("debate.detailOpenClaw"),
        state: hasOpenClaw ? (hasHermes || hasFinal ? "done" : "active") : mode === "debate" && streaming ? "active" : "idle",
      },
      {
        title: t("debate.stepHermes"),
        detail: t("debate.detailHermes"),
        state: hasHermes ? (hasFinal ? "done" : "active") : mode === "debate" && (hasOpenClaw || streaming) ? "queued" : "idle",
      },
      {
        title: t("debate.stepFinal"),
        detail: t("debate.detailFinal"),
        state: hasFinal ? "active" : mode === "debate" && (hasOpenClaw || hasHermes || streaming) ? "queued" : "idle",
      },
    ] as const;
  }, [mode, streamStatusEntries, streaming, t]);

  function syncTextareaHeight() {
    const target = textareaRef.current;
    if (!target) return;
    target.style.height = "auto";
    target.style.height = `${Math.min(target.scrollHeight, 160)}px`;
  }

  function appendPrompt(snippet: string) {
    setInput((current) => {
      const next = `${current}${current.trim() ? "\n\n" : ""}${snippet}`;
      requestAnimationFrame(() => {
        textareaRef.current?.focus();
        syncTextareaHeight();
      });
      return next;
    });
  }

  function toggleFocusMode() {
    setFocusMode((current) => {
      const next = !current;
      if (next) {
        setShowConversationRail(false);
        setShowContextRail(false);
        setShowWorkspaceOverview(false);
        setShowConversationSummary(false);
        setShowDebateWorkflow(false);
      }
      return next;
    });
  }

  const scheduleStreamFlush = useCallback(() => {
    if (flushRafRef.current !== null) return;
    flushRafRef.current = requestAnimationFrame(() => {
      flushRafRef.current = null;
      setStreamBuffers({ ...streamBuffersRef.current });
      setStreamStatuses({ ...streamStatusesRef.current });
    });
  }, []);

  const loadFilePreview = useCallback(async (path: string) => {
    setSelectedFilePath(path);
    setContextTab("files");
    setFilePreviewLoading(true);
    setFilePreviewError("");
    try {
      const result = await projectsApi.fileContent(id, path);
      setSelectedFileContent(result.content);
    } catch (err) {
      setSelectedFileContent("");
      setFilePreviewError(err instanceof Error ? err.message : "Failed to load file preview");
    } finally {
      setFilePreviewLoading(false);
    }
  }, [id]);

  const selectConv = useCallback(async (conv: Conversation) => {
    setActiveConv(conv);
    setMode(conv.mode);
    const data = await convsApi.get(id, conv.id);
    setMessages(data.messages);
  }, [id]);

  /** Roadmap → workspace deep-link. Switches to the workspace tab, opens the
   *  conversation that owns the message, and scrolls to it once messages
   *  load (handled by the effect below via a pending-scroll ref + tick). */
  const pendingScrollMessageIdRef = useRef<string | null>(null);
  const [pendingScrollTick, setPendingScrollTick] = useState(0);
  const openSourceMessage = useCallback(async (conversationId: string, messageId: string) => {
    setProjectTab("workspace");
    pendingScrollMessageIdRef.current = messageId;
    setPendingScrollTick((tick) => tick + 1);
    const target = convs.find((c) => c.id === conversationId);
    if (target && (!activeConv || activeConv.id !== conversationId)) {
      await selectConv(target);
    } else if (!target) {
      // conv not in current list — refetch and try again
      try {
        const list = await convsApi.list(id);
        setConvs(list);
        const found = list.find((c) => c.id === conversationId);
        if (found) await selectConv(found);
      } catch { /* best-effort */ }
    }
  }, [convs, activeConv, selectConv, id]);

  /** Roadmap → workspace dispatch. The Roadmap drawer creates a new conv
   *  and gets back a pre-built prompt; we switch tabs, open the conv,
   *  and pre-fill the composer so the user can review then hit Send. */
  const onDispatched = useCallback(async (conversationId: string, prompt: string) => {
    setProjectTab("workspace");
    try {
      const list = await convsApi.list(id);
      setConvs(list);
      const target = list.find((c) => c.id === conversationId);
      if (target) await selectConv(target);
    } catch { /* best-effort */ }
    setInput(prompt);
    // Make sure the composer scrolls into view; users typically expect to
    // see the prompt waiting for them.
    requestAnimationFrame(() => {
      textareaRef.current?.focus();
      textareaRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
    });
  }, [id, selectConv]);

  useEffect(() => {
    const pendingScrollMessageId = pendingScrollMessageIdRef.current;
    if (!pendingScrollMessageId) return;
    if (!messages.some((m) => m.id === pendingScrollMessageId)) return;
    const el = document.querySelector<HTMLDivElement>(`[data-message-id="${pendingScrollMessageId}"]`);
    if (el) {
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      el.classList.add("ring-2", "ring-[#0050A0]");
      window.setTimeout(() => el.classList.remove("ring-2", "ring-[#0050A0]"), 2200);
    }
    pendingScrollMessageIdRef.current = null;
  }, [messages, pendingScrollTick]);

  useEffect(() => {
    setShowAppSidebar(!focusMode);
    return () => {
      setShowAppSidebar(true);
    };
  }, [focusMode, setShowAppSidebar]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const p = await projectsApi.get(id);
      const [files, branchData, status, profiles] = await Promise.all([
        projectsApi.fileTree(id).catch(() => []),
        p.source_type === "git"
          ? projectsApi.gitBranches(id).catch(() => ({ branches: [] }))
          : Promise.resolve({ branches: [] }),
        p.source_type === "git"
          ? projectsApi.gitStatus(id).catch(() => null)
          : Promise.resolve(null),
        agentProfilesApi.list().catch(() => []),
      ]);
      if (cancelled) return;
      setProject(p);
      setFileTree(files);
      setBranches(branchData.branches);
      setGitStatus(status);
      setAgentProfiles(profiles.filter((profile) => profile.enabled));
    })();
    return () => { cancelled = true; };
  }, [id]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const cs = await convsApi.list(id);
      if (cancelled) return;
      setConvs(cs);
      streamBuffersRef.current = {};
      streamStatusesRef.current = {};
      setStreamBuffers({});
      setStreamStatuses({});
      setStreaming(false);

      const currentId = activeConv?.id;
      const preferred = cs.find((conv) => conv.id === currentId) ?? cs[0] ?? null;
      setActiveConv(preferred);
      if (preferred) {
        setMode(preferred.mode);
        const data = await convsApi.get(id, preferred.id);
        if (!cancelled) setMessages(data.messages);
      } else {
        setMessages([]);
      }
    })();
    return () => { cancelled = true; };
  }, [id, activeConv?.id]);

  // Pull the cached per-conversation summary whenever the active conversation
  // changes, AND once more after streaming finishes (the backend refreshes
  // the cache row at the end of each turn). Failures are silent — the
  // summary chip simply doesn't appear.
  useEffect(() => {
    let cancelled = false;
    const conv = activeConv;
    queueMicrotask(() => {
      void (async () => {
        if (!conv) {
          if (!cancelled) setConvSummary(null);
          return;
        }
        try {
          const s = await convsApi.summary(id, conv.id);
          if (!cancelled) setConvSummary(s);
        } catch {
          if (!cancelled) setConvSummary(null);
        }
      })();
    });
    return () => { cancelled = true; };
  }, [id, activeConv, streaming]);

  useEffect(() => {
    if (!activeConv) {
      safeCloseWs(wsRef.current);
      wsRef.current = null;
      return;
    }

    const convId = activeConv.id;
    const ws = createWsConnection(convId, id);
    wsRef.current = ws;

    ws.onmessage = (e) => {
      const evt = JSON.parse(e.data) as {
        type: string;
        agent?: string;
        content?: string;
        round?: number;
        phase?: string;
        message?: string;
      };

      if (evt.type === "status" && evt.agent && evt.message) {
        const label = displayAgentName(evt.agent, evt.round, evt.phase);
        streamStatusesRef.current[label] = {
          agent: evt.agent,
          message: evt.message,
          round: evt.round,
          phase: evt.phase,
          startedAt: Date.now(),
        };
        setStatusNow(Date.now());
        setStreaming(true);
        if (!shouldAutoScrollRef.current) setShowJumpToBottom(true);
        scheduleStreamFlush();
        return;
      }

      if (evt.type === "error") {
        const errorText = evt.message ?? "Agent error";
        setMessages((ms) => [...ms, {
          id: crypto.randomUUID(),
          conversation_id: convId,
          role: "system",
          content: errorText,
          agent_name: "System",
          created_at: new Date().toISOString(),
        } as Message]);
        streamBuffersRef.current = {};
        streamStatusesRef.current = {};
        if (flushRafRef.current !== null) {
          cancelAnimationFrame(flushRafRef.current);
          flushRafRef.current = null;
        }
        setStreamBuffers({});
        setStreamStatuses({});
        setStreaming(false);
        return;
      }

      if (!evt.agent) return;
      const label = displayAgentName(evt.agent, evt.round, evt.phase);
      if (evt.type === "chunk" && evt.content) {
        delete streamStatusesRef.current[label];
        streamBuffersRef.current[label] = (streamBuffersRef.current[label] ?? "") + evt.content;
        if (!shouldAutoScrollRef.current) setShowJumpToBottom(true);
        scheduleStreamFlush();
      } else if (evt.type === "done") {
        delete streamStatusesRef.current[label];
        const buffered = streamBuffersRef.current[label] ?? "";
        if (buffered) {
          const role = evt.agent.startsWith("Hermes") ? "hermes" : "openclaw";
          setMessages((ms) => [...ms, {
            id: crypto.randomUUID(),
            conversation_id: convId,
            role,
            content: buffered,
            agent_name: label,
            created_at: new Date().toISOString(),
          } as Message]);
        }
        delete streamBuffersRef.current[label];
        scheduleStreamFlush();
        if (Object.keys(streamBuffersRef.current).length === 0 && Object.keys(streamStatusesRef.current).length === 0) {
          setStreaming(false);
          // Re-fetch conversation messages so client-side placeholder UUIDs are
          // replaced with the real DB IDs. Without this, "Add to Roadmap"
          // would send a non-existent source_message_id and the FK constraint
          // on project_tasks.source_message_id => messages.id would 500.
          (async () => {
            try {
              const data = await convsApi.get(id, convId);
              setMessages(data.messages);
            } catch {
              // best-effort; if refetch fails, the fake-UUID rows stay until
              // the next conversation switch
            }
          })();
        }
      }
    };

    ws.onclose = () => {
      if (wsRef.current === ws) setStreaming(false);
    };

    ws.onerror = () => {
      if (wsRef.current !== ws) return;
      setMessages((ms) => [...ms, {
        id: crypto.randomUUID(),
        conversation_id: convId,
        role: "system",
        content: "WebSocket connection error",
        agent_name: "System",
        created_at: new Date().toISOString(),
      } as Message]);
      streamStatusesRef.current = {};
      setStreamStatuses({});
      setStreaming(false);
    };

    return () => {
      safeCloseWs(ws);
      if (wsRef.current === ws) wsRef.current = null;
      if (flushRafRef.current !== null) {
        cancelAnimationFrame(flushRafRef.current);
        flushRafRef.current = null;
      }
    };
  }, [activeConv, id, scheduleStreamFlush, wsReconnectKey]);

  useEffect(() => {
    if (!shouldAutoScrollRef.current) return;
    if (scrollRafRef.current !== null) return;
    scrollRafRef.current = requestAnimationFrame(() => {
      scrollRafRef.current = null;
      // Scroll only the messages region — never call scrollIntoView, which
      // would walk up the DOM and scroll the outer <main>, pushing the
      // project header out of view.
      const el = messagesScrollRef.current;
      if (!el) return;
      const behavior: ScrollBehavior = streaming ? "auto" : "smooth";
      el.scrollTo({ top: el.scrollHeight, behavior });
    });
    return () => {
      if (scrollRafRef.current !== null) {
        cancelAnimationFrame(scrollRafRef.current);
        scrollRafRef.current = null;
      }
    };
  }, [messages, streamBuffers, streamStatuses, streaming]);

  useEffect(() => {
    if (!streaming || Object.keys(streamStatuses).length === 0) return;
    const timer = window.setInterval(() => setStatusNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [streaming, streamStatuses]);

  function handleMessagesScroll() {
    const el = messagesScrollRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    const nearBottom = distanceFromBottom < 96;
    shouldAutoScrollRef.current = nearBottom;
    setShowJumpToBottom(!nearBottom && (streaming || Object.keys(streamBuffersRef.current).length > 0));
  }

  function jumpToBottom() {
    shouldAutoScrollRef.current = true;
    setShowJumpToBottom(false);
    const el = messagesScrollRef.current;
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  }

  /**
   * Open the new-conversation type chooser. Before committing the create
   * we refresh `agentProfiles` so freshly-added agents (Gemini, Claude…)
   * show up immediately without forcing a full page reload.
   */
  async function newConv() {
    try {
      const profiles = await agentProfilesApi.list();
      setAgentProfiles(profiles.filter((profile) => profile.enabled));
    } catch {
      // Best-effort refresh; if it fails we still open the modal with the
      // previously-loaded set so the user can pick a core agent.
    }
    setShowNewConvModal(true);
  }

  /**
   * Actually create the conversation. `chosenMode` may be any `ChatMode`:
   * - core ("openclaw" | "hermes" | "debate") goes straight to the DB
   *   `mode` column (matches the enum backend accepts in mig 0001).
   * - "agent:<id>" / "agents:<id>,<id>" are UI-only encodings; the DB
   *   row is created with "openclaw" as a placeholder and the live agent
   *   selection is carried by each WS turn's `mode` payload.
   * See backlog #10 for the longer-term plan to make conv.mode reflect
   * the live selection too.
   */
  async function actuallyCreateConv(chosenMode: ChatMode) {
    const createMode = isCoreAgentMode(chosenMode) ? chosenMode : "openclaw";
    const conv = await convsApi.create(
      id,
      `${modeLabel(chosenMode, agentProfiles, t)} Conversation ${convs.length + 1}`,
      createMode,
    );
    setConvs((cs) => [conv, ...cs]);
    setMessages([]);
    setActiveConv(conv);
    setMode(chosenMode);
    pushToast({
      tone: "success",
      title: "Conversation created",
      description: `${modeLabel(chosenMode, agentProfiles, t)} is ready for the next turn.`,
    });
  }

  async function refreshProject() {
    if (refreshing) return;
    setRefreshing(true);
    setRefreshStatus("");
    try {
      const p = await projectsApi.get(id);
      let syncMsg = "Workspace refreshed";
      if (p.source_type === "git") {
        try {
          const sync = await projectsApi.gitSync(id);
          syncMsg = sync.status === "fast-forwarded"
            ? "Pulled latest from origin"
            : sync.status === "up-to-date"
              ? "Already up to date"
              : "No matching remote branch";
        } catch (err) {
          syncMsg = err instanceof Error ? err.message : "Sync failed";
        }
      }
      const [files, branchData, status] = await Promise.all([
        projectsApi.fileTree(id).catch(() => []),
        p.source_type === "git"
          ? projectsApi.gitBranches(id).catch(() => ({ branches: [] }))
          : Promise.resolve({ branches: [] }),
        p.source_type === "git"
          ? projectsApi.gitStatus(id).catch(() => null)
          : Promise.resolve(null),
      ]);
      setProject(p);
      setFileTree(files);
      setBranches(branchData.branches);
      setGitStatus(status);
      setRefreshStatus(syncMsg);
      pushToast({ tone: p.source_type === "git" ? "info" : "success", title: "Workspace refreshed", description: syncMsg });
      setTimeout(() => setRefreshStatus(""), 3000);
    } finally {
      setRefreshing(false);
    }
  }

  async function deleteConv(conv: Conversation, e: MouseEvent) {
    e.stopPropagation();
    if (!confirm(`Delete conversation "${conv.title}"? All messages will be lost.`)) return;
    // Hit the backend first, then update local state only on success. The
    // previous flow optimistically removed the conv and let the runtime
    // overlay surface unrelated errors (e.g. "Project not found" when the
    // caller's role is too low to delete) as crashes.
    try {
      await convsApi.delete(id, conv.id);
    } catch (err) {
      const message = err instanceof Error ? err.message : "Unknown error";
      pushToast({
        tone: "error",
        title: "Could not delete conversation",
        description: message,
      });
      return;
    }
    const remaining = convs.filter((c) => c.id !== conv.id);
    setConvs(remaining);
    if (activeConv?.id === conv.id) {
      if (remaining.length > 0) {
        setActiveConv(remaining[0]);
        setMode(remaining[0].mode);
        const data = await convsApi.get(id, remaining[0].id);
        setMessages(data.messages);
      } else {
        setActiveConv(null);
        setMessages([]);
      }
    }
    pushToast({ tone: "warning", title: "Conversation deleted", description: `Removed ${conv.title} from this workspace.` });
  }

  function sendViaWs(content: string) {
    if (!activeConv) return;
    const ws = wsRef.current;
    const convId = activeConv.id;

    shouldAutoScrollRef.current = true;
    setShowJumpToBottom(false);
    streamBuffersRef.current = {};
    streamStatusesRef.current = {};
    setStreamBuffers({});
    setStreamStatuses({});
    setStreaming(true);
    setMessages((ms) => [...ms, {
      id: crypto.randomUUID(),
      conversation_id: convId,
      role: "user",
      content,
      created_at: new Date().toISOString(),
    } as Message]);

    const payload = JSON.stringify({ type: "message", content, mode });
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(payload);
    } else if (ws && ws.readyState === WebSocket.CONNECTING) {
      ws.addEventListener("open", () => ws.send(payload), { once: true });
    } else {
      setWsReconnectKey((k) => k + 1);
      setTimeout(() => {
        const next = wsRef.current;
        if (!next) return;
        if (next.readyState === WebSocket.OPEN) next.send(payload);
        else next.addEventListener("open", () => next.send(payload), { once: true });
      }, 0);
    }
  }

  function stopStreaming() {
    wsRef.current?.close();
    streamBuffersRef.current = {};
    streamStatusesRef.current = {};
    setStreamBuffers({});
    setStreamStatuses({});
    setStreaming(false);
    setWsReconnectKey((k) => k + 1);
  }

  async function switchBranch(branch: string) {
    if (!project || project.source_type !== "git" || !branch || branch === project.default_branch) return;
    setSwitchingBranch(true);
    setRefreshStatus("");
    try {
      const updated = await projectsApi.checkoutBranch(id, branch);
      const [files, branchData, status] = await Promise.all([
        projectsApi.fileTree(id).catch(() => []),
        projectsApi.gitBranches(id).catch(() => ({ branches: [] })),
        projectsApi.gitStatus(id).catch(() => null),
      ]);
      setProject(updated);
      setFileTree(files);
      setBranches(branchData.branches);
      setGitStatus(status);
      setSelectedFilePath("");
      setSelectedFileContent("");
      streamBuffersRef.current = {};
      streamStatusesRef.current = {};
      setStreamBuffers({});
      setStreamStatuses({});
      setRefreshStatus(`Switched to ${branch}`);
      pushToast({ tone: "success", title: t("toast.branchSwitched"), description: t("toast.branchSwitchedDesc").replace("{branch}", branch) });
      setTimeout(() => setRefreshStatus(""), 3000);
    } catch (err) {
      const msg = err instanceof Error ? err.message : t("toast.branchSwitchFailed");
      setRefreshStatus(`Switch failed: ${msg}`);
      pushToast({ tone: "error", title: t("toast.branchSwitchFailed"), description: msg });
      setTimeout(() => setRefreshStatus(""), 6000);
    } finally {
      setSwitchingBranch(false);
    }
  }

  function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!input.trim() || streaming) return;
    sendViaWs(input.trim());
    setInput("");
    requestAnimationFrame(syncTextareaHeight);
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e as unknown as FormEvent);
    }
  }

  return (
    <div className="flex h-screen min-h-0 flex-col bg-[radial-gradient(circle_at_top,_#F8FBFF_0%,_#F5F7FB_38%,_#EEF3F8_100%)]">
      <div className="border-b border-[#E2E8F0] bg-white px-5 py-4">
        <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
          <div className="flex items-start gap-3">
            <button
              onClick={() => router.push("/projects")}
              className="mt-0.5 rounded-lg border border-[#E2E8F0] bg-white p-2 text-[#64748B] transition hover:border-[#0050A0] hover:text-[#0050A0]"
              title={t("project.backToProjects")}
            >
              <ArrowLeft size={16} />
            </button>
            <div>
              <div className="flex flex-wrap items-center gap-2">
                <h1 className="type-section-title text-[1.6rem]">{project?.name ?? t("project.workspace")}</h1>
                {project && <StatusPill>{project.source_type === "git" ? t("project.gitRepository") : project.source_type === "upload" ? t("project.uploadProject") : t("project.localFolder")}</StatusPill>}
                <StatusPill className={modeStyle(mode)}>{modeLabel(mode, agentProfiles, t)}</StatusPill>
                {focusMode && <StatusPill className="bg-[#EAF2FF] text-[#0050A0]">{t("chat.focusMode")}</StatusPill>}
              </div>
              <p className="mt-2 type-body-muted">
                {t("project.repoSummary")}
              </p>
              <div className="mt-3 flex flex-wrap items-center gap-2 text-[13px] text-[#64748B]">
                <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-1.5">Branch: {project?.default_branch ?? "—"}</span>
                <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-1.5">Conversations: {convs.length}</span>
                <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-1.5">Dirty files: {dirtyCount}</span>
                {selectedFilePath && <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-1.5">Focused file: {selectedFilePath.split("/").pop() ?? selectedFilePath}</span>}
              </div>
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2 xl:justify-end">
            <Button variant="secondary" size="sm" onClick={() => setShowConversationRail((value) => !value)}>
              {showConversationRail ? t("chat.hideHistory") : t("chat.showHistory")}
            </Button>
            <Button variant="secondary" size="sm" onClick={() => setShowContextRail((value) => !value)}>
              {showContextRail ? t("chat.hideContext") : t("chat.showContext")}
            </Button>
            <Button variant="subtle" size="sm" onClick={() => setShowWorkspaceOverview((value) => !value)}>
              {showWorkspaceOverview ? t("chat.hideDetails") : t("chat.showDetails")}
            </Button>
            <Button variant={focusMode ? "tonal" : "subtle"} size="sm" onClick={toggleFocusMode}>
              {focusMode ? t("chat.exitFocusMode") : t("chat.focusMode")}
            </Button>
            <Button variant="secondary" size="sm" onClick={newConv}>
              <Plus size={14} /> {t("project.newConversation")}
            </Button>
            <Button variant="secondary" size="sm" onClick={refreshProject} loading={refreshing}>
              <RefreshCw size={14} /> {t("project.refreshWorkspace")}
            </Button>
          </div>
        </div>

        {showWorkspaceOverview && (
          <div className="mt-4 grid grid-cols-2 gap-3 xl:grid-cols-6">
            <OverviewCard label={t("project.currentBranch")} value={project?.default_branch ?? "—"} icon={<GitBranch size={14} />} />
            <OverviewCard label={t("project.conversations")} value={String(convs.length)} icon={<MessageSquarePlus size={14} />} />
            <OverviewCard label={t("project.dirtyFiles")} value={String(dirtyCount)} icon={<File size={14} />} tone={dirtyCount > 0 ? "warning" : "default"} />
            <OverviewCard label={t("project.mode")} value={modeLabel(mode, agentProfiles, t)} icon={<Sparkles size={14} />} />
            <OverviewCard label={t("project.selectedFile")} value={selectedFilePath ? selectedFilePath.split("/").pop() ?? selectedFilePath : "—"} icon={<FolderOpen size={14} />} />
            <OverviewCard label={t("project.lastUpdate")} value={project ? formatDate(project.updated_at) : "—"} icon={<Clock3 size={14} />} />
          </div>
        )}

        {refreshStatus && (
          <div className="mt-3">
            <InlineBanner
              tone={refreshStatus.toLowerCase().includes("failed") ? "error" : "info"}
              title={t("toast.workspaceStatus")}
              description={refreshStatus}
            />
          </div>
        )}
      </div>

      {/* Project-level tabs: Workspace = chat & context, Insights/Cost/Roadmap = analytics dashboards */}
      <div className="border-b border-[#E2E8F0] bg-white px-6">
        <div className="flex gap-1">
          {([
            ["workspace", t("project.workspace"), MessageSquarePlus],
            ["insights", t("project.insights"), PieChart],
            ["cost", t("project.cost"), DollarSign],
            ["roadmap", t("project.roadmap"), MapIcon],
          ] as Array<[ProjectTab, string, typeof MessageSquarePlus]>).map(([key, label, Icon]) => (
            <button
              key={key}
              onClick={() => setProjectTab(key)}
              className={cn(
                "-mb-px flex items-center gap-1.5 border-b-2 px-3 py-2.5 text-xs font-medium transition-colors",
                projectTab === key
                  ? "border-[#0050A0] text-[#0050A0]"
                  : "border-transparent text-[#64748B] hover:text-[#1A1A2E]"
              )}
            >
              <Icon size={13} /> {label}
            </button>
          ))}
        </div>
      </div>

      {projectTab === "insights" && <InsightsTab projectId={id} />}
      {projectTab === "cost" && <CostTab projectId={id} />}
      {projectTab === "roadmap" && <RoadmapTab projectId={id} onOpenSource={openSourceMessage} onDispatched={onDispatched} />}

      {projectTab === "workspace" && (
      <div className="flex min-h-0 flex-1 bg-[#F8FAFC]">
        {showConversationRail && !focusMode && (
        <aside className="flex w-[300px] flex-shrink-0 flex-col border-r border-[#E2E8F0] bg-white/96">
          <div className="flex-shrink-0 border-b border-[#E2E8F0] px-4 py-4">
            <div className="flex items-start justify-between gap-3">
              <div>
                <p className="text-[12px] font-semibold tracking-[0.06em] text-[#94A3B8]">{t("convList.title")}</p>
                <p className="mt-2 type-body-muted">{t("convList.subtitle")}</p>
              </div>
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  onClick={() => setShowConversationRail(false)}
                  className="rounded-lg border border-[#E2E8F0] p-2 text-[#64748B] hover:border-[#0050A0] hover:text-[#0050A0]"
                  title="Collapse history"
                >
                  <ChevronRight size={14} className="rotate-180" />
                </button>
                <button onClick={newConv} className="rounded-lg border border-[#E2E8F0] p-2 text-[#64748B] hover:border-[#0050A0] hover:text-[#0050A0]">
                  <Plus size={14} />
                </button>
              </div>
            </div>
            <div className="mt-4 grid grid-cols-3 gap-2">
              <MiniStat label={t("convList.all")} value={String(conversationHealth.total)} tone="blue" />
              <MiniStat label={t("convList.recent")} value={String(conversationHealth.recent)} tone="green" />
              <MiniStat label="Debate" value={String(conversationHealth.debate)} tone="amber" />
            </div>
            <div className="mt-4 flex items-center gap-2 rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-2">
              <Search size={14} className="text-[#94A3B8]" />
              <input
                value={conversationQuery}
                onChange={(e) => setConversationQuery(e.target.value)}
                placeholder={t("convList.searchPlaceholder")}
                className="w-full bg-transparent text-[15px] text-[#1A1A2E] outline-none placeholder:text-[#94A3B8]"
              />
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-auto px-2 py-2">
            {!project ? (
              <div className="space-y-2 px-2 py-2">
                <SkeletonBlock className="h-[88px] w-full" />
                <SkeletonBlock className="h-[88px] w-full" />
                <SkeletonBlock className="h-[88px] w-full" />
              </div>
            ) : filteredConvs.length === 0 ? (
              <SectionEmpty
                className="px-4 py-8"
                title={conversationQuery ? t("convList.noMatches") : t("convList.empty")}
                description={conversationQuery
                  ? "Try another title or mode keyword, or create a fresh conversation."
                  : "Create a conversation to start building shared history across agents."}
                action={<Button size="sm" onClick={newConv}><Plus size={14} /> New Conversation</Button>}
              />
            ) : (
              filteredConvs.map((conv) => (
                <div
                  key={conv.id}
                  className={cn(
                    "group mb-2 rounded-2xl border p-3 transition",
                    activeConv?.id === conv.id
                      ? "border-[#BFDBFE] bg-[#EFF6FF] shadow-sm"
                      : "border-transparent bg-transparent hover:border-[#E2E8F0] hover:bg-[#F8FAFC]"
                  )}
                >
                  <div className="flex items-start gap-3">
                    <button className="min-w-0 flex-1 text-left" onClick={() => void selectConv(conv)}>
                      <div className="flex items-center gap-2">
                        <MessageSquarePlus size={14} className={activeConv?.id === conv.id ? "text-[#0050A0]" : "text-[#94A3B8]"} />
                        <span className="truncate text-[15px] font-medium tracking-[-0.01em] text-[#1A1A2E]">{conv.title}</span>
                        {/* Active / streaming pills removed — the row already
                            highlights the selected conversation via background
                            colour, and the streaming state has its own
                            indicator in the chat header. */}
                      </div>
                      <div className="mt-2 flex flex-wrap items-center gap-2 text-[12px] text-[#64748B]">
                        {(() => {
                          // Custom Debate / Custom Agent conversations land in
                          // DB as mode="openclaw" because of the mig 0002 CHECK
                          // constraint; infer the real intent from the
                          // auto-generated title until backlog #25 widens the
                          // constraint and we can store the actual mode.
                          const inferred = inferConversationMode(conv, agentProfiles);
                          const style = inferred?.className ?? MODE_STYLES[conv.mode];
                          const label = inferred?.label ?? MODE_LABELS[conv.mode];
                          return (
                            <span className={cn("rounded-full px-2 py-0.5", style)}>{label}</span>
                          );
                        })()}
                        <span>{formatRelativeTime(conv.updated_at)}</span>
                      </div>
                      <div className="mt-2 line-clamp-2 text-[13px] leading-6 text-[#64748B]">
                        {activeConv?.id === conv.id
                          ? t("convDesc.threadSummary").replace("{count}", String(activeThreadSummary.messageCount))
                            + (activeThreadSummary.lastAgent
                              ? t("convDesc.lastAgentSuffix").replace("{agent}", activeThreadSummary.lastAgent)
                              : "")
                          : conv.mode === "debate"
                            ? t("convDesc.debate")
                            : conv.mode === "hermes"
                              ? t("convDesc.hermes")
                              : t("convDesc.openclaw")}
                      </div>
                    </button>
                    {/* Show delete only when the viewer authored the conv OR
                        owns the project. Admins-on-the-project-but-not-owner
                        currently lose the UI affordance; backend still
                        accepts their request, so they can fall back to the
                        API. Fixing this fully needs `effective_role` on the
                        Project response — tracked in deferred backlog. */}
                    {(conv.user_id === currentUserId || project?.user_id === currentUserId) && (
                      <button
                        type="button"
                        onClick={(e) => void deleteConv(conv, e)}
                        title="Delete conversation"
                        className="opacity-0 transition group-hover:opacity-100 text-[#94A3B8] hover:text-[#C8102E]"
                      >
                        <Trash2 size={13} />
                      </button>
                    )}
                  </div>
                </div>
              ))
            )}
          </div>
        </aside>
        )}

        <main className="flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="border-b border-[#E2E8F0] bg-white/85 px-5 py-4 backdrop-blur-md">
            <div className="mx-auto flex w-full max-w-[72rem] flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
              <div className="min-w-0 space-y-3">
                <div>
                  <div className="flex flex-wrap items-center gap-2">
                    {!showConversationRail && !focusMode && (
                      <Button variant="secondary" size="sm" onClick={() => setShowConversationRail(true)}>
                        <ChevronRight size={14} /> {t("convList.title")}
                      </Button>
                    )}
                    <h2 className="truncate text-[18px] font-semibold tracking-[-0.02em] text-[#1A1A2E]">{activeConv?.title ?? t("chat.placeholderEmpty")}</h2>
                    {activeConv && <StatusPill className={modeStyle(activeConv.mode)}>{modeLabel(activeConv.mode, agentProfiles, t)}</StatusPill>}
                    {streaming && <StatusPill className="bg-[#EFF6FF] text-[#1D4ED8]">{t("chat.streaming")}</StatusPill>}
                  </div>
                  <p className="mt-1 text-xs text-[#64748B]">
                    {activeConv ? t("chat.chooseStrategy") : t("convList.emptyDesc")}
                  </p>
                </div>

                {activeConv && (
                  <div className="flex flex-wrap items-center gap-2 text-[13px] text-[#64748B]">
                    <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-1.5">Thread: {activeThreadSummary.messageCount} messages</span>
                    <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-1.5">Last agent: {activeThreadSummary.lastAgent ?? "Waiting for first reply"}</span>
                    <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-1.5">Last user: {activeThreadSummary.lastUserAt ? formatRelativeTime(activeThreadSummary.lastUserAt) : "Not yet"}</span>
                    <button
                      type="button"
                      onClick={() => setShowConversationSummary((value) => !value)}
                      className="rounded-full border border-[#E2E8F0] bg-white px-3 py-1.5 text-[13px] text-[#475569] transition hover:border-[#94A3B8] hover:text-[#1A1A2E]"
                    >
                      {showConversationSummary ? "Hide details" : "Show details"}
                    </button>
                  </div>
                )}

                {showConversationSummary && activeConv && (
                  // Outer space-y-2 stacks the 3-card grid above the per-conv
                  // summary chip (fsc-only addition). Typography inside the
                  // cards follows Hermes's [12px]/[14px] scale.
                  <div className="space-y-2">
                    <div className="grid gap-3 sm:grid-cols-3">
                      <div className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE] px-4 py-3">
                        <div className="text-[12px] font-semibold tracking-[0.05em] text-[#94A3B8]">Thread size</div>
                        <div className="mt-1.5 text-[14px] font-semibold leading-6 text-[#1A1A2E]">{activeThreadSummary.messageCount} messages</div>
                      </div>
                      <div className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE] px-4 py-3">
                        <div className="text-[12px] font-semibold tracking-[0.05em] text-[#94A3B8]">Last agent</div>
                        <div className="mt-1 truncate text-[14px] font-semibold leading-6 text-[#1A1A2E]">{activeThreadSummary.lastAgent ?? "Waiting for first reply"}</div>
                      </div>
                      <div className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE] px-4 py-3">
                        <div className="text-[12px] font-semibold tracking-[0.05em] text-[#94A3B8]">Last user turn</div>
                        <div className="mt-1.5 text-[14px] font-semibold leading-6 text-[#1A1A2E]">{activeThreadSummary.lastUserAt ? formatRelativeTime(activeThreadSummary.lastUserAt) : "Not yet"}</div>
                      </div>
                    </div>

                    {/* LLM-generated per-conversation summary. Backend refreshes
                        this after each turn; the chip is hidden until the first
                        successful refresh produces a row. */}
                    {convSummary && convSummary.summary && (
                      <div className="rounded-2xl border border-[#DBEAFE] bg-[#EFF6FF] px-4 py-3">
                        <div className="flex items-center justify-between gap-2">
                          <div className="text-[12px] font-semibold uppercase tracking-[0.08em] text-[#1D4ED8]">Conversation summary</div>
                          <div className="text-[11px] text-[#64748B]">
                            {formatRelativeTime(convSummary.updated_at)} · {convSummary.source_message_count} msgs
                          </div>
                        </div>
                        <p className="mt-1.5 whitespace-pre-wrap text-[13px] leading-6 text-[#1E3A8A]">{convSummary.summary}</p>
                        {convSummary.highlights.length > 0 && (
                          <ul className="mt-2 list-disc space-y-1 pl-5 text-[12px] leading-5 text-[#1E3A8A]">
                            {convSummary.highlights.map((h, i) => (
                              <li key={i}>{h}</li>
                            ))}
                          </ul>
                        )}
                        {convSummary.keywords.length > 0 && (
                          <div className="mt-2.5 flex flex-wrap gap-1.5">
                            {convSummary.keywords.map((k) => (
                              <span key={k} className="rounded-full bg-white px-2 py-0.5 text-[11px] font-medium tracking-[-0.005em] text-[#1D4ED8] ring-1 ring-[#BFDBFE]">
                                {k}
                              </span>
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                )}

                <div className="flex flex-wrap items-center gap-2">
                  {(["openclaw", "hermes", "debate"] as AgentMode[]).map((candidate) => (
                    <button
                      key={candidate}
                      onClick={() => setMode(candidate)}
                      className={cn(
                        "rounded-xl px-3 py-2 text-xs font-medium transition",
                        mode === candidate
                          ? MODE_STYLES[candidate]
                          : "border border-[#E2E8F0] bg-white text-[#64748B] hover:border-[#94A3B8] hover:text-[#1A1A2E]"
                      )}
                    >
                      <span className="inline-flex items-center gap-1.5">
                        {candidate === "openclaw" && <Cpu size={12} />}
                        {candidate === "hermes" && <Bot size={12} />}
                        {candidate === "debate" && <Zap size={12} />}
                        {MODE_LABELS[candidate]}
                      </span>
                    </button>
                  ))}
                  {agentProfiles.map((profile) => {
                    const candidate = `agent:${profile.id}` as ChatMode;
                    return (
                      <button
                        key={profile.id}
                        onClick={() => setMode(candidate)}
                        className={cn(
                          "rounded-xl px-3 py-2 text-xs font-medium transition",
                          mode === candidate
                            ? modeStyle(candidate)
                            : "border border-[#E2E8F0] bg-white text-[#64748B] hover:border-emerald-200 hover:text-emerald-700"
                        )}
                        title={`${profile.provider} / ${profile.model}`}
                      >
                        <span className="inline-flex items-center gap-1.5">
                          <Sparkles size={12} />
                          {profile.name}
                        </span>
                      </button>
                    );
                  })}
                  {/* Always available now that the picker accepts the two
                      built-in agents as participants. */}
                  {true && (
                    <button
                      key="custom-debate"
                      onClick={() => setShowCustomDebatePicker(true)}
                      className={cn(
                        "rounded-xl px-3 py-2 text-xs font-medium transition",
                        mode.startsWith("agents:")
                          ? modeStyle(mode)
                          : "border border-[#E2E8F0] bg-white text-[#64748B] hover:border-teal-200 hover:text-teal-700"
                      )}
                      title={t("chat.customDebateHint")}
                    >
                      <span className="inline-flex items-center gap-1.5">
                        <Zap size={12} />
                        {mode.startsWith("agents:") ? (() => {
                          const ids = mode.slice("agents:".length).split(",");
                          return `${t("chat.customDebateLabel")} (${ids.length})`;
                        })() : t("chat.customDebateConfigure")}
                      </span>
                    </button>
                  )}
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-2 xl:max-w-[360px] xl:justify-end">
                <Button
                  variant="subtle"
                  size="sm"
                  onClick={() => selectedFilePath && appendPrompt(`Please analyze file: ${selectedFilePath}\nFocus on architecture, risks, and recommended edits.`)}
                  disabled={!selectedFilePath || streaming}
                >
                  {t("chat.useFocusedFile")}
                </Button>
                <Button
                  variant="subtle"
                  size="sm"
                  onClick={() => appendPrompt(`Review the current branch state for ${project?.default_branch ?? "this workspace"}. Summarize changed files, risks, and the best next step.`)}
                  disabled={streaming}
                >
                  {t("chat.reviewBranch")}
                </Button>
                {!showContextRail && !focusMode && (
                  <Button variant="secondary" size="sm" onClick={() => setShowContextRail(true)}>
                    <ChevronRight size={14} /> {t("project.contextPanel")}
                  </Button>
                )}
                {mode === "debate" && (
                  <Button variant="subtle" size="sm" onClick={() => setShowDebateWorkflow((value) => !value)}>
                    {showDebateWorkflow ? t("chat.hideDebateFlow") : t("chat.showDebateFlow")}
                  </Button>
                )}
              </div>
            </div>

            {mode === "debate" && showDebateWorkflow && (
              <div className="mt-4 rounded-3xl border border-[#FDE68A] bg-[linear-gradient(180deg,#FFFDF5_0%,#FFFBEB_100%)] p-5 text-[14px] leading-7 text-[#92400E] shadow-sm">
                <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                  <div>
                    <div className="font-semibold text-[#92400E]">Debate workflow</div>
                    <p className="mt-2 text-[13px] leading-6 text-[#A16207]">Structured disagreement first, synthesis second. Use this when trade-offs or correctness matter more than speed.</p>
                  </div>
                  <StatusPill className="bg-white/90 text-[#B45309]">{streaming ? "Debate running" : "Ready for next round"}</StatusPill>
                </div>
                <div className="mt-4 grid gap-3 lg:grid-cols-3">
                  {debateTimeline.map((step, index) => (
                    <DebateStepCard key={step.title} index={index + 1} title={step.title} detail={step.detail} state={step.state} />
                  ))}
                </div>
              </div>
            )}
          </div>

          <div
            ref={messagesScrollRef}
            onScroll={handleMessagesScroll}
            className="relative flex-1 overflow-auto px-4 py-7 sm:px-6"
          >
            {!activeConv ? (
              <div className="flex h-full items-center justify-center">
                <SectionEmpty
                  className="w-full max-w-lg bg-white px-6 py-12 shadow-sm"
                  title="Create a conversation to start"
                  description="Use OpenClaw, Hermes, or Debate Mode with the same project context and repository state."
                  action={<Button onClick={newConv}><Plus size={14} /> New Conversation</Button>}
                />
              </div>
            ) : (
              <div className="mx-auto w-full max-w-[72rem] space-y-6">
                {streaming && (
                  <div className="max-w-[56rem] rounded-3xl border border-[#DBEAFE] bg-[linear-gradient(180deg,#FFFFFF_0%,#F8FBFF_42%,#EFF6FF_100%)] px-5 py-4 shadow-[0_16px_40px_rgba(59,130,246,0.08)]">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div>
                        <div className="text-[15px] font-semibold tracking-[-0.01em] text-[#1D4ED8]">Live agent activity</div>
                        <div className="mt-1 text-xs leading-6 text-[#64748B]">
                          {mode === "debate"
                            ? "Debate mode streams partial reasoning from each side before synthesis."
                            : "The active model is streaming its response into this conversation."}
                        </div>
                      </div>
                      <StatusPill className="bg-white text-[#1D4ED8]">{streamStatusEntries.length || streamBufferEntries.length} active lane(s)</StatusPill>
                    </div>
                  </div>
                )}

                {messages.map((msg) => (
                  <div
                    key={msg.id}
                    data-message-id={msg.id}
                    style={{ contentVisibility: "auto", containIntrinsicSize: "0 200px" }}
                    className="rounded-[24px] transition-shadow"
                  >
                    <ChatMessage message={msg} projectId={id} currentUserId={currentUserId} />
                  </div>
                ))}

                {streamStatusEntries.map(([agentLabel, status]) => (
                  <StatusMessage key={`streaming-status-${agentLabel}`} label={agentLabel} status={status} now={statusNow} />
                ))}

                {streamBufferEntries.map(([agentLabel, content]) =>
                  content ? (
                    <ChatMessage
                      key={`streaming-buffer-${agentLabel}`}
                      projectId={id}
                      message={{
                        id: `streaming-buffer-${agentLabel}`,
                        conversation_id: "",
                        role: agentLabel.startsWith("Hermes") ? "hermes" : "openclaw",
                        content,
                        agent_name: agentLabel,
                        created_at: new Date().toISOString(),
                      }}
                      streaming
                    />
                  ) : null
                )}
                <div ref={bottomRef} />
              </div>
            )}

            {showJumpToBottom && (
              <button
                type="button"
                onClick={jumpToBottom}
                className="sticky bottom-3 left-1/2 -translate-x-1/2 rounded-full border border-[#BFDBFE] bg-white px-3.5 py-2 text-[13px] font-medium text-[#0050A0] shadow-sm hover:bg-blue-50"
              >
                <span className="inline-flex items-center gap-1.5">
                  <ArrowDown size={13} /> New output
                </span>
              </button>
            )}
          </div>

          <div className="border-t border-[#E2E8F0] bg-white/92 px-5 py-4 shadow-[0_-10px_30px_rgba(15,23,42,0.04)] backdrop-blur-md">
            <form onSubmit={handleSubmit} className="mx-auto w-full max-w-[72rem] space-y-3">
              <div className="flex flex-wrap items-center gap-2 text-[13px] text-[#64748B]">
                <span className={cn("rounded-full px-2.5 py-1", modeStyle(mode))}>{modeLabel(mode, agentProfiles, t)}</span>
                {selectedFilePath ? (
                  <button
                    type="button"
                    onClick={() => {
                      setSelectedFilePath("");
                      setSelectedFileContent("");
                    }}
                    className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-1.5 text-[13px] text-[#475569] transition hover:border-[#94A3B8]"
                  >
                    Focused file: {focusedFileName} ×
                  </button>
                ) : (
                  <span className="rounded-full border border-dashed border-[#CBD5E1] bg-white px-3 py-1.5 text-[13px] text-[#94A3B8]">{t("chat.noFocusedFile")}</span>
                )}
                {streaming && <span className="rounded-full border border-[#BFDBFE] bg-[#EFF6FF] px-2.5 py-1 text-[#1D4ED8]">Agents are responding…</span>}
                <button
                  type="button"
                  onClick={() => setShowComposerTools((value) => !value)}
                  className="rounded-full border border-[#E2E8F0] bg-white px-3 py-1.5 text-[13px] text-[#475569] transition hover:border-[#94A3B8] hover:text-[#1A1A2E]"
                >
                  {showComposerTools ? t("chat.hideSuggestions") : t("chat.showSuggestions")}
                </button>
              </div>

              <div className="rounded-[28px] border border-[#D6DFEA] bg-[linear-gradient(180deg,#FFFFFF_0%,#FBFCFE_100%)] p-3 shadow-[0_18px_50px_rgba(15,23,42,0.08)]">
                {showComposerTools && (
                  <div className="mb-3 flex flex-wrap gap-2">
                    {QUICK_ACTIONS.map(({ key, labelKey, icon: Icon, prompt }) => (
                      <button
                        key={key}
                        type="button"
                        onClick={() => appendPrompt(prompt)}
                        disabled={streaming || (key === "patch" && !activeConv)}
                        className="rounded-full border border-[#E2E8F0] bg-white px-3.5 py-1.5 text-[13px] font-medium text-[#475569] transition hover:border-[#0050A0] hover:text-[#0050A0] disabled:cursor-not-allowed disabled:opacity-50"
                      >
                        <span className="inline-flex items-center gap-1.5">
                          <Icon size={12} />
                          {t(labelKey)}
                        </span>
                      </button>
                    ))}
                    <button
                      type="button"
                      onClick={() => selectedFilePath && appendPrompt(`Please analyze file: ${selectedFilePath}\nExplain purpose, important logic, and likely change points.`)}
                      disabled={!selectedFilePath || streaming}
                      className="rounded-full border border-[#E2E8F0] bg-white px-3.5 py-1.5 text-[13px] font-medium text-[#475569] transition hover:border-[#0050A0] hover:text-[#0050A0] disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      {t("chat.analyzeFocused")}
                    </button>
                  </div>
                )}

                <div className="flex items-end gap-3">
                  <textarea
                    ref={textareaRef}
                    value={input}
                    onChange={(e) => {
                      setInput(e.target.value);
                      requestAnimationFrame(syncTextareaHeight);
                    }}
                    onKeyDown={handleKeyDown}
                    placeholder={activeConv ? t("chat.placeholder") : t("chat.placeholderEmpty")}
                    disabled={!activeConv || streaming}
                    rows={1}
                    className={cn(
                      "min-h-[68px] max-h-44 flex-1 resize-none overflow-auto rounded-[24px] border border-[#D6DFEA] bg-white px-4 py-3 text-[13px] leading-6 text-[#1A1A2E]",
                      "placeholder:text-[#94A3B8] focus:border-transparent focus:outline-none focus:ring-2 focus:ring-[#0050A0]",
                      "disabled:cursor-not-allowed disabled:opacity-50"
                    )}
                    style={{ height: "auto" }}
                  />
                  <div className="flex flex-col items-end gap-2">
                    {streaming ? (
                      <Button type="button" variant="secondary" onClick={stopStreaming}>
                        <Square size={14} /> {t("chat.stop")}
                      </Button>
                    ) : (
                      <Button type="submit" disabled={!activeConv || !input.trim()}>
                        <Send size={15} /> {t("chat.send")}
                      </Button>
                    )}
                    <div className="text-[12px] text-[#94A3B8]">{input.trim().length} {t("chat.chars")}</div>
                  </div>
                </div>
              </div>
            </form>
          </div>
        </main>

        {showContextRail && !focusMode && (
        <aside className="flex w-[320px] flex-shrink-0 flex-col border-l border-[#E2E8F0] bg-white/96">
          <div className="flex-shrink-0 border-b border-[#E2E8F0] px-4 py-4">
            <div className="flex items-center justify-between gap-3">
              <div>
                <p className="text-[12px] font-semibold tracking-[0.06em] text-[#94A3B8]">{t("context.title")}</p>
              </div>
              <button
                type="button"
                onClick={() => setShowContextRail(false)}
                className="rounded-lg border border-[#E2E8F0] p-2 text-[#64748B] hover:border-[#0050A0] hover:text-[#0050A0]"
                title={t("chat.collapseContext")}
              >
                <ChevronRight size={14} />
              </button>
            </div>
            <div className="mt-3 flex gap-2">
              {([
                ["files", t("project.tabFiles")],
                ["git", t("project.tabGit")],
                ["project", t("project.tabProject")],
              ] as [ContextTab, string][]).map(([tab, label]) => (
                <button
                  key={tab}
                  onClick={() => setContextTab(tab)}
                  className={cn(
                    "rounded-xl px-3 py-2 text-xs font-medium transition",
                    contextTab === tab
                      ? "bg-[#EAF2FF] text-[#0050A0]"
                      : "text-[#64748B] hover:bg-[#F8FAFC] hover:text-[#1A1A2E]"
                  )}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-auto p-4">
            {contextTab === "files" && (
              <div className="space-y-4">
                <section className="rounded-2xl border border-[#E2E8F0] bg-white p-4 shadow-sm">
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <h3 className="text-sm font-semibold text-[#1A1A2E]">{t("context.quickLookup")}</h3>
                      <p className="mt-1 text-xs text-[#64748B]">{t("context.lookupDesc")}</p>
                    </div>
                    <StatusPill>{allFilePaths.length} {t("context.files")}</StatusPill>
                  </div>
                  <div className="mt-3 flex items-center gap-2 rounded-xl border border-[#E2E8F0] bg-[#F8FAFC] px-3 py-2">
                    <Search size={14} className="text-[#94A3B8]" />
                    <input
                      value={fileQuery}
                      onChange={(e) => setFileQuery(e.target.value)}
                      placeholder={t("context.findPlaceholder")}
                      className="w-full bg-transparent text-[15px] text-[#1A1A2E] outline-none placeholder:text-[#94A3B8]"
                    />
                  </div>
                  <div className="mt-3 space-y-2">
                    {fileHits.length === 0 ? (
                      <SectionEmpty title={t("contextFiles.noMatches")} description={t("contextFiles.noMatchesDesc")} />
                    ) : fileHits.map((path) => (
                      <button
                        key={path}
                        type="button"
                        onClick={() => void loadFilePreview(path)}
                        className={cn(
                          "flex w-full items-center justify-between rounded-xl border px-3 py-2 text-left text-sm transition",
                          selectedFilePath === path
                            ? "border-[#BFDBFE] bg-[#EFF6FF] text-[#0050A0]"
                            : "border-[#E2E8F0] bg-[#FBFCFE] text-[#475569] hover:border-[#94A3B8] hover:text-[#1A1A2E]"
                        )}
                      >
                        <span className="truncate">{path}</span>
                        <span className="ml-3 text-[11px] text-[#94A3B8]">{t("context.open")}</span>
                      </button>
                    ))}
                  </div>
                </section>

                <section className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE]">
                  <div className="border-b border-[#E2E8F0] px-4 py-3">
                    <div className="flex items-center justify-between gap-3">
                      <div>
                        <h3 className="text-sm font-semibold text-[#1A1A2E]">{t("contextFiles.projectFiles")}</h3>
                        <p className="mt-1 text-xs text-[#64748B]">{t("contextFiles.projectFilesDesc")}</p>
                      </div>
                      <button
                        type="button"
                        onClick={refreshProject}
                        disabled={refreshing}
                        className="rounded-lg border border-[#E2E8F0] p-2 text-[#64748B] hover:border-[#0050A0] hover:text-[#0050A0]"
                      >
                        <RefreshCw size={13} className={refreshing ? "animate-spin" : ""} />
                      </button>
                    </div>
                  </div>
                  <div className="max-h-[260px] overflow-auto py-2">
                    {fileTree.length === 0 ? (
                      <SectionEmpty
                        className="mx-4 my-4"
                        title={t("contextFiles.noFilesAvailable")}
                        description={t("contextFiles.noFilesAvailableDesc")}
                      />
                    ) : fileTree.map((node) => (
                      <FileNodeItem
                        key={node.path}
                        node={node}
                        depth={0}
                        selectedPath={selectedFilePath}
                        onSelect={loadFilePreview}
                      />
                    ))}
                  </div>
                </section>

                <section className="rounded-2xl border border-[#E2E8F0] bg-white p-4">
                  <div className="flex items-center justify-between gap-2">
                    <div>
                      <h3 className="text-sm font-semibold text-[#1A1A2E]">{t("contextFiles.filePreview")}</h3>
                      <p className="mt-1 text-xs text-[#64748B] truncate">{selectedFilePath || t("contextFiles.noFileSelected")}</p>
                    </div>
                    {selectedFilePath && (
                      <button
                        type="button"
                        onClick={() => appendPrompt(t("contextFiles.askAboutFilePrompt").replace("{path}", selectedFilePath))}
                        className="rounded-lg border border-[#E2E8F0] px-2.5 py-1.5 text-xs font-medium text-[#0050A0] hover:border-[#0050A0]"
                      >
                        {t("contextFiles.askAboutFile")}
                      </button>
                    )}
                  </div>

                  {filePreviewLoading ? (
                    <div className="mt-4 space-y-3">
                      <SkeletonBlock className="h-4 w-40" />
                      <SkeletonBlock className="h-56 w-full rounded-xl" />
                    </div>
                  ) : filePreviewError ? (
                    <InlineBanner title={t("contextFiles.previewFailed")} description={filePreviewError} tone="error" />
                  ) : selectedFilePath ? (
                    <div className="mt-4 overflow-hidden rounded-xl border border-[#E2E8F0]">
                      <SyntaxHighlighter language={detectLanguage(selectedFilePath)}>{selectedFileContent}</SyntaxHighlighter>
                    </div>
                  ) : (
                    <SectionEmpty
                      className="mt-4"
                      title={t("contextFiles.noFileSelected")}
                      description={t("contextFiles.previewDesc")}
                    />
                  )}
                </section>
              </div>
            )}

            {contextTab === "git" && (
              <div className="space-y-4">
                <section className="grid grid-cols-3 gap-3">
                  <MiniStat label={t("gitStatus.changed")} value={String(gitStatus?.changed.length ?? 0)} tone="blue" />
                  <MiniStat label={t("gitStatus.staged")} value={String(gitStatus?.staged.length ?? 0)} tone="green" />
                  <MiniStat label={t("gitStatus.untracked")} value={String(gitStatus?.untracked.length ?? 0)} tone="amber" />
                </section>

                <section className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE] p-4 shadow-sm">
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <h3 className="text-sm font-semibold text-[#1A1A2E]">{t("gitStatus.hotspots")}</h3>
                      <p className="mt-1 text-xs text-[#64748B]">{t("gitStatus.hotspotsDesc")}</p>
                    </div>
                    <button
                      type="button"
                      onClick={() => appendPrompt(t("gitStatus.askAgentsPrompt"))}
                      className="rounded-lg border border-[#E2E8F0] px-2.5 py-1.5 text-xs font-medium text-[#0050A0] hover:border-[#0050A0]"
                    >
                      {t("gitStatus.askAgents")}
                    </button>
                  </div>
                  <div className="mt-3 space-y-2">
                    {gitHotspots.length === 0 ? (
                      <SectionEmpty title={t("gitStatus.noHotspots")} description={t("gitStatus.noHotspotsDesc")} />
                    ) : gitHotspots.map((path, index) => (
                      <button
                        key={path}
                        type="button"
                        onClick={() => void loadFilePreview(path)}
                        className="flex w-full items-center justify-between rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-left text-sm text-[#475569] transition hover:border-[#0050A0] hover:text-[#0050A0]"
                      >
                        <span className="flex min-w-0 items-center gap-2">
                          <span className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full bg-[#EFF6FF] text-[11px] font-semibold text-[#1D4ED8]">{index + 1}</span>
                          <span className="truncate">{path}</span>
                        </span>
                        <span className="text-[11px] text-[#94A3B8]">{t("gitStatus.preview")}</span>
                      </button>
                    ))}
                  </div>
                </section>

                <section className="rounded-2xl border border-[#E2E8F0] bg-white p-4">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <h3 className="text-sm font-semibold text-[#1A1A2E]">{t("gitStatus.branchControl")}</h3>
                      <p className="mt-1 text-xs text-[#64748B]">{t("gitStatus.branchControlDesc")}</p>
                    </div>
                    {project?.source_type === "git" && (
                      <select
                        value={project.default_branch ?? ""}
                        disabled={switchingBranch || streaming}
                        onChange={(e) => void switchBranch(e.target.value)}
                        className="h-9 rounded-lg border border-[#E2E8F0] bg-white px-3 text-xs text-[#1A1A2E] disabled:opacity-50"
                      >
                        {(branches.length ? branches : [project.default_branch ?? "main"]).map((branch) => (
                          <option key={branch} value={branch}>{branch}</option>
                        ))}
                      </select>
                    )}
                  </div>
                  {switchingBranch && <p className="mt-2 text-xs text-[#64748B]">{t("gitStatus.switching")}</p>}
                </section>

                <GitStatusSection title={t("gitStatus.changedFiles")} items={gitStatus?.changed ?? []} onOpen={loadFilePreview} />
                <GitStatusSection title={t("gitStatus.stagedFiles")} items={gitStatus?.staged ?? []} onOpen={loadFilePreview} />
                <GitStatusSection title={t("gitStatus.untrackedFiles")} items={gitStatus?.untracked ?? []} onOpen={loadFilePreview} />
              </div>
            )}

            {contextTab === "project" && project && (
              <div className="space-y-4">
                <section className="rounded-2xl border border-[#E2E8F0] bg-white p-4">
                  <h3 className="text-sm font-semibold text-[#1A1A2E]">{t("projectInfo.workspaceSummary")}</h3>
                  <dl className="mt-4 space-y-3 text-sm">
                    <InfoRow label={t("projectInfo.projectName")} value={project.name} />
                    <InfoRow label={t("projectInfo.sourceType")} value={project.source_type} />
                    <InfoRow label={t("projectInfo.sourcePath")} value={project.source_path} />
                    <InfoRow label={t("projectInfo.localPath")} value={project.local_path ?? "—"} />
                    <InfoRow label={t("projectInfo.defaultBranch")} value={project.default_branch ?? "—"} />
                    <InfoRow label={t("projectInfo.updated")} value={formatDate(project.updated_at)} />
                  </dl>
                </section>

                <section className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE] p-4 shadow-sm">
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <h3 className="text-sm font-semibold text-[#1A1A2E]">{t("projectInfo.intelligence")}</h3>
                      <p className="mt-1 text-xs text-[#64748B]">{t("projectInfo.intelligenceDesc")}</p>
                    </div>
                    <StatusPill>{workspaceInsights.length === 1 ? t("projectInfo.insightOne") : t("projectInfo.insightMany").replace("{n}", String(workspaceInsights.length))}</StatusPill>
                  </div>
                  <div className="mt-3 space-y-3">
                    {workspaceInsights.length === 0 ? (
                      <SectionEmpty title={t("projectInfo.noInsights")} description={t("projectInfo.noInsightsDesc")} />
                    ) : workspaceInsights.map((insight) => (
                      <button
                        key={insight.title}
                        type="button"
                        onClick={() => appendPrompt(insight.action)}
                        className="w-full rounded-2xl border border-[#E2E8F0] bg-white px-4 py-3 text-left transition hover:border-[#0050A0] hover:shadow-sm"
                      >
                        <div className="text-sm font-semibold text-[#1A1A2E]">{insight.title}</div>
                        <div className="mt-1 text-xs text-[#64748B]">{insight.detail}</div>
                        <div className="mt-3 text-xs font-medium text-[#0050A0]">{t("projectInfo.useAsPrompt")}</div>
                      </button>
                    ))}
                  </div>
                </section>

                <section className="rounded-2xl border border-[#E2E8F0] bg-[#F8FAFC] p-4">
                  <h3 className="text-sm font-semibold text-[#1A1A2E]">{t("projectInfo.howToUse")}</h3>
                  <ul className="mt-3 list-disc space-y-2 pl-5 text-sm text-[#64748B]">
                    <li>{t("projectInfo.bullet1")}</li>
                    <li>{t("projectInfo.bullet2")}</li>
                    <li>{t("projectInfo.bullet3")}</li>
                  </ul>
                </section>
              </div>
            )}
          </div>
        </aside>
        )}
      </div>
      )}

      {showCustomDebatePicker && (
        <CustomDebatePicker
          profiles={agentProfiles}
          initialMode={mode}
          onClose={() => {
            setShowCustomDebatePicker(false);
            setPickerCreatesConv(false);
          }}
          onConfirm={(picked) => {
            const debateMode = `agents:${picked.join(",")}` as ChatMode;
            setShowCustomDebatePicker(false);
            if (pickerCreatesConv) {
              setPickerCreatesConv(false);
              void actuallyCreateConv(debateMode);
            } else {
              setMode(debateMode);
            }
          }}
        />
      )}

      {showNewConvModal && (
        <NewConversationModal
          profiles={agentProfiles}
          onClose={() => setShowNewConvModal(false)}
          onPickSingle={(picked) => {
            setShowNewConvModal(false);
            void actuallyCreateConv(picked);
          }}
          onPickCustomDebate={() => {
            // Hand off to the existing 2–4 agent picker; the picker's
            // confirm callback will fall through to actuallyCreateConv
            // because pickerCreatesConv is set here.
            setShowNewConvModal(false);
            setPickerCreatesConv(true);
            setShowCustomDebatePicker(true);
          }}
        />
      )}
    </div>
  );
}

/**
 * Debate participant picker. Originally custom-only ("C6"), but as of this
 * commit also accepts the built-in OpenClaw + Hermes as participants so a
 * user without any of their own agent profiles can still drive a focused
 * 2-agent debate, or mix-and-match (e.g. OpenClaw + Hermes + Gemini).
 *
 * Encoding: tokens are either the literal `"openclaw"` / `"hermes"` or a
 * UUID of an enabled agent profile. The backend's
 * `ws.rs::agent_mode_from_str` performs the same parsing — kept in sync.
 */
function CustomDebatePicker({
  profiles,
  initialMode,
  onClose,
  onConfirm,
}: {
  profiles: AgentProfile[];
  initialMode: ChatMode;
  onClose: () => void;
  onConfirm: (ids: string[]) => void;
}) {
  const t = useT();
  const presetIds = initialMode.startsWith("agents:")
    ? initialMode.slice("agents:".length).split(",")
    : [];
  const [picked, setPicked] = useState<string[]>(presetIds);

  // Built-in pseudo-profiles. Their `id` is the reserved literal accepted
  // by the backend parser; provider/model are shown for parity with custom
  // rows so the UI looks consistent. provider="built-in" is used as a
  // marker for the BUILT-IN badge — translated below.
  const builtinRows = [
    { id: "openclaw", name: "OpenClaw", provider: "built-in", model: "gpt-5.5 (OpenClaw gateway)" },
    { id: "hermes",   name: "Hermes",   provider: "built-in", model: "hermes-agent (Hermes gateway)" },
  ];
  const allRows: { id: string; name: string; provider: string; model: string }[] = [
    ...builtinRows,
    ...profiles.map((p) => ({ id: p.id, name: p.name, provider: p.provider, model: p.model })),
  ];

  function toggle(id: string) {
    setPicked((prev) => {
      if (prev.includes(id)) return prev.filter((x) => x !== id);
      if (prev.length >= 4) return prev;       // hard cap — matches backend
      return [...prev, id];
    });
  }
  function moveUp(id: string) {
    setPicked((prev) => {
      const i = prev.indexOf(id);
      if (i <= 0) return prev;
      const next = prev.slice();
      [next[i - 1], next[i]] = [next[i], next[i - 1]];
      return next;
    });
  }

  const ready = picked.length >= 2 && picked.length <= 4;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-6" onClick={onClose}>
      <div className="w-full max-w-lg rounded-2xl bg-white shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="border-b border-[#E2E8F0] px-5 py-4">
          <h3 className="type-card-title">{t("chat.customDebateTitle")}</h3>
          <p className="type-body-muted mt-1">{t("chat.customDebateDesc")}</p>
        </div>
        <div className="max-h-[420px] overflow-y-auto px-5 py-3 space-y-1.5">
          {allRows.map((p) => {
            const order = picked.indexOf(p.id);
            const selected = order >= 0;
            const isBuiltin = p.provider === "built-in";
            return (
              <label
                key={p.id}
                className={`flex cursor-pointer items-center gap-2 rounded-xl border px-3 py-2.5 transition ${
                  selected ? "border-[#0050A0] bg-[#EFF6FF]" : "border-[#E2E8F0] hover:border-[#94A3B8]"
                }`}
              >
                <input
                  type="checkbox"
                  checked={selected}
                  onChange={() => toggle(p.id)}
                  className="h-3.5 w-3.5"
                />
                {selected && (
                  <span className="rounded-full bg-[#0050A0] px-1.5 text-[11px] font-semibold text-white">
                    #{order + 1}
                  </span>
                )}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5">
                    <span className="truncate text-[14px] font-medium tracking-[-0.01em] text-[#1A1A2E]">{p.name}</span>
                    {isBuiltin && (
                      <span className="rounded-full bg-[#F1F5F9] px-1.5 py-0.5 text-[10px] font-semibold tracking-[0.04em] text-[#64748B]">
                        {t("agents.badgeBuiltIn")}
                      </span>
                    )}
                  </div>
                  <div className="text-[12px] leading-5 text-[#64748B]">{p.provider} · {p.model}</div>
                </div>
                {selected && order > 0 && (
                  <button
                    type="button"
                    onClick={(e) => { e.preventDefault(); moveUp(p.id); }}
                    className="text-[12px] text-[#0050A0] hover:underline"
                    title={t("chat.customDebateMoveUp")}
                  >
                    ↑
                  </button>
                )}
              </label>
            );
          })}
        </div>
        <div className="flex items-center justify-between gap-2 border-t border-[#E2E8F0] px-5 py-3 text-xs">
          <span className="text-[#64748B]">
            {picked.length} / 4 {t("chat.customDebateSelected")}
            {picked.length > 0 && picked.length < 2 && ` · ${t("chat.customDebateMinHint")}`}
          </span>
          <div className="flex gap-2">
            <button type="button" onClick={onClose} className="text-[#64748B] hover:text-[#1A1A2E]">{t("common.cancel")}</button>
            <button
              type="button"
              disabled={!ready}
              onClick={() => onConfirm(picked)}
              className="rounded-md bg-[#0050A0] px-3 py-1.5 font-medium text-white hover:bg-[#003B7A] disabled:bg-[#94A3B8]"
            >
              {t("chat.customDebateConfirm")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * "+ New Conversation" type chooser. Sequencing rule:
 * - Single-agent cards (OpenClaw / Hermes / each enabled custom agent)
 *   commit immediately on click via onPickSingle, no second step.
 * - "Debate Mode" card commits immediately as the legacy OpenClaw + Hermes
 *   debate (backend's hardcoded two-agent orchestration).
 * - "Custom Debate" card hands off to the existing CustomDebatePicker for
 *   the 2–4 agent multi-select; the picker's confirm path is what creates
 *   the conversation in that branch.
 *
 * The list of custom agents comes from `agentProfiles`, which the parent
 * re-fetches just before opening this modal so newly-added agents appear
 * without a page reload.
 */
function NewConversationModal({
  profiles,
  onClose,
  onPickSingle,
  onPickCustomDebate,
}: {
  profiles: AgentProfile[];
  onClose: () => void;
  onPickSingle: (mode: ChatMode) => void;
  onPickCustomDebate: () => void;
}) {
  const t = useT();
  // Custom Debate is always available now that the picker accepts OpenClaw
  // and Hermes as participants — even a user with zero custom profiles can
  // run an OpenClaw + Hermes debate through this code path.
  const canCustomDebate = true;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/35 p-6 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-full max-w-2xl rounded-[24px] bg-white shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="border-b border-[#E2E8F0] px-6 py-5">
          <h3 className="type-card-title">{t("chat.newConvModalTitle")}</h3>
          <p className="type-body-muted mt-1">{t("chat.newConvModalDesc")}</p>
        </div>

        <div className="max-h-[60vh] space-y-5 overflow-y-auto px-6 py-5">
          {/* Single-agent section: 2 built-ins + each enabled custom profile */}
          <section>
            <div className="type-overline mb-2.5">{t("chat.singleAgentSection")}</div>
            <div className="grid gap-2.5 sm:grid-cols-2">
              <AgentChooserCard
                label={t("chat.modeOpenClaw")}
                detail={t("agents.descOpenClaw")}
                tone="blue"
                icon={<Cpu size={16} />}
                onClick={() => onPickSingle("openclaw")}
              />
              <AgentChooserCard
                label={t("chat.modeHermes")}
                detail={t("agents.descHermes")}
                tone="violet"
                icon={<Bot size={16} />}
                onClick={() => onPickSingle("hermes")}
              />
              {profiles.map((profile) => (
                <AgentChooserCard
                  key={profile.id}
                  label={profile.name}
                  detail={`${profile.provider} · ${profile.model}`}
                  tone="emerald"
                  icon={<Sparkles size={16} />}
                  onClick={() => onPickSingle(`agent:${profile.id}` as ChatMode)}
                />
              ))}
            </div>
            {profiles.length === 0 && (
              <p className="type-meta mt-2">{t("chat.customAgentTip")}</p>
            )}
          </section>

          {/* Multi-agent: built-in two-agent debate + custom 2–4 debate */}
          <section>
            <div className="type-overline mb-2.5">{t("chat.multiAgentSection")}</div>
            <div className="grid gap-2.5 sm:grid-cols-2">
              <AgentChooserCard
                label={t("chat.modeDebate")}
                detail={t("chat.debateModeShortDesc")}
                tone="amber"
                icon={<Zap size={16} />}
                onClick={() => onPickSingle("debate")}
              />
              <AgentChooserCard
                label={t("chat.modeCustomDebate")}
                detail={t("chat.customDebateShortDesc")}
                tone="teal"
                icon={<Zap size={16} />}
                onClick={onPickCustomDebate}
              />
            </div>
          </section>
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-[#E2E8F0] px-6 py-4">
          <button
            type="button"
            onClick={onClose}
            className="rounded-xl px-3.5 py-2 text-[13px] font-medium tracking-[-0.01em] text-[#64748B] transition hover:bg-[#F1F5F9] hover:text-[#1A1A2E]"
          >
            {t("common.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}

function AgentChooserCard({
  label,
  detail,
  tone,
  icon,
  onClick,
  disabled,
}: {
  label: string;
  detail: string;
  tone: "blue" | "violet" | "amber" | "teal" | "emerald";
  icon: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
}) {
  // Tone-specific accents reused from the main mode toggle row so the
  // chooser visually matches what the user will see in the composer
  // after the conversation is created.
  const toneCx: Record<typeof tone, string> = {
    blue:    "border-[#BFDBFE] bg-[#EFF6FF] text-[#0050A0]",
    violet:  "border-[#DDD6FE] bg-[#F5F3FF] text-[#7C3AED]",
    amber:   "border-[#FDE68A] bg-[#FFFBEB] text-[#B45309]",
    teal:    "border-teal-200 bg-teal-50 text-teal-700",
    emerald: "border-emerald-200 bg-emerald-50 text-emerald-700",
  };
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "flex items-start gap-3 rounded-2xl border p-3.5 text-left transition shadow-[0_1px_2px_rgba(15,23,42,0.03)]",
        disabled
          ? "cursor-not-allowed border-[#E2E8F0] bg-[#F8FAFC] opacity-60"
          : "border-[#E2E8F0] bg-white hover:-translate-y-px hover:border-[#94A3B8] hover:shadow-[0_8px_18px_rgba(15,23,42,0.06)]",
      )}
    >
      <div className={cn("mt-0.5 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-xl border", toneCx[tone])}>
        {icon}
      </div>
      <div className="min-w-0">
        <div className="text-[14px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">{label}</div>
        <div className="mt-1 text-[12px] leading-5 text-[#64748B]">{detail}</div>
      </div>
    </button>
  );
}

function DebateStepCard({
  index,
  title,
  detail,
  state,
}: {
  index: number;
  title: string;
  detail: string;
  state: "idle" | "queued" | "active" | "done";
}) {
  const t = useT();
  const toneClass = state === "done"
    ? "border-[#FDE68A] bg-white"
    : state === "active"
      ? "border-[#F59E0B] bg-[#FFF7ED]"
      : state === "queued"
        ? "border-[#FDE68A] bg-[#FEFCE8]"
        : "border-[#FDE68A]/60 bg-white/70";

  const badgeClass = state === "done"
    ? "bg-[#FEF3C7] text-[#92400E]"
    : state === "active"
      ? "bg-[#F59E0B] text-white"
      : state === "queued"
        ? "bg-[#FFF7ED] text-[#B45309]"
        : "bg-white text-[#A16207]";

  const statusLabel = state === "done"
    ? t("debate.statusDone")
    : state === "active"
      ? t("debate.statusRunning")
      : state === "queued"
        ? t("debate.statusQueued")
        : t("debate.statusWaiting");

  return (
    <div className={cn("rounded-2xl border p-4 shadow-sm", toneClass)}>
      <div className="flex items-center justify-between gap-3">
        <span className="text-xs font-semibold uppercase tracking-[0.14em] text-[#A16207]">Step {index}</span>
        <span className={cn("rounded-full px-2.5 py-1 text-[11px] font-semibold", badgeClass)}>{statusLabel}</span>
      </div>
      <div className="mt-3 text-sm font-semibold text-[#92400E]">{title}</div>
      <div className="mt-1 text-xs text-[#A16207]">{detail}</div>
    </div>
  );
}

function OverviewCard({
  label,
  value,
  icon,
  tone = "default",
}: {
  label: string;
  value: string;
  icon: ReactNode;
  tone?: "default" | "warning";
}) {
  return (
    <div className={cn(
      "rounded-2xl border bg-white px-4 py-3 shadow-sm",
      tone === "warning" ? "border-[#FDE68A] bg-[#FFFBEB]" : "border-[#E2E8F0]"
    )}>
      <div className="flex items-center gap-2 text-xs text-[#64748B]">
        {icon}
        {label}
      </div>
      <div className="mt-2 truncate text-sm font-semibold text-[#1A1A2E]">{value}</div>
    </div>
  );
}

function StatusPill({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span className={cn("rounded-full bg-[#F1F5F9] px-2.5 py-1 text-xs font-medium text-[#475569]", className)}>
      {children}
    </span>
  );
}

function MiniStat({ label, value, tone }: { label: string; value: string; tone: "blue" | "green" | "amber" }) {
  const toneClass = tone === "blue"
    ? "border-[#BFDBFE] bg-[#EFF6FF] text-[#1D4ED8]"
    : tone === "green"
      ? "border-[#BBF7D0] bg-[#F0FDF4] text-[#166534]"
      : "border-[#FDE68A] bg-[#FFFBEB] text-[#B45309]";

  return (
    <div className={cn("rounded-2xl border px-4 py-3", toneClass)}>
      <div className="text-xs font-medium">{label}</div>
      <div className="mt-1 text-lg font-semibold">{value}</div>
    </div>
  );
}

function GitStatusSection({ title, items, onOpen }: { title: string; items: string[]; onOpen: (path: string) => void }) {
  const t = useT();
  return (
    <section className="rounded-2xl border border-[#E2E8F0] bg-white p-4">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold text-[#1A1A2E]">{title}</h3>
        <span className="text-xs text-[#94A3B8]">{items.length}</span>
      </div>
      {items.length === 0 ? (
        <SectionEmpty
          className="mt-3 px-4 py-6"
          title={t("gitStatus.noFiles")}
          description={t("gitStatus.noFilesDesc")}
        />
      ) : (
        <div className="mt-3 space-y-2">
          {items.map((item) => (
            <button
              key={item}
              onClick={() => void onOpen(item)}
              className="flex w-full items-center justify-between rounded-xl border border-[#E2E8F0] px-3 py-2 text-left text-sm text-[#475569] hover:border-[#0050A0] hover:text-[#0050A0]"
            >
              <span className="truncate">{item}</span>
              <File size={13} className="flex-shrink-0" />
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[120px_1fr] gap-3">
      <dt className="text-[#64748B]">{label}</dt>
      <dd className="break-all text-[#1A1A2E]">{value}</dd>
    </div>
  );
}

function StatusMessage({ label, status, now }: { label: string; status: StreamStatus; now: number }) {
  const isHermes = status.agent.startsWith("Hermes") || label.startsWith("Hermes");
  const elapsed = Math.max(0, Math.floor(((now || status.startedAt) - status.startedAt) / 1000));
  const phaseLabel = status.phase === "final"
    ? "Final"
    : status.round
      ? `Round ${status.round}`
      : "Streaming";

  return (
    <div className="flex gap-3">
      <div className={cn(
        "flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full text-xs font-bold text-white shadow-sm",
        isHermes ? "bg-[#7C3AED]" : "bg-[#0050A0]"
      )}>
        {isHermes ? <Bot size={14} /> : <Cpu size={14} />}
      </div>
      <div className="max-w-[52rem] min-w-0">
        <div className="mb-1 flex flex-wrap items-center gap-2">
          <span className={cn(
            "block text-[12px] font-semibold tracking-[-0.01em]",
            isHermes ? "text-[#7C3AED]" : "text-[#0050A0]"
          )}>
            {label}
            <span className="ml-1 animate-pulse">●</span>
          </span>
          <span className="rounded-full border border-white/70 bg-white/70 px-2 py-0.5 text-[11px] font-medium text-[#64748B]">{phaseLabel}</span>
        </div>
        <div className={cn(
          "rounded-2xl rounded-tl-sm border px-4 py-3 text-sm shadow-sm",
          isHermes
            ? "border-[#E9D5FF] bg-[#F5F3FF] text-[#1A1A2E]"
            : "border-[#BFDBFE] bg-[#EFF6FF] text-[#1A1A2E]"
        )}>
          <div className="flex items-start gap-2">
            <span className={cn(
              "mt-0.5 h-3 w-3 flex-shrink-0 rounded-full border-2 border-t-transparent animate-spin",
              isHermes ? "border-[#7C3AED]" : "border-[#0050A0]"
            )} />
            <div className="min-w-0 flex-1">
              <div>{status.message}</div>
              <div className="mt-2 text-xs text-[#64748B]">Elapsed {elapsed}s</div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

const ChatMessage = memo(function ChatMessage({ message, projectId, streaming, currentUserId }: { message: Message; projectId: string; streaming?: boolean; currentUserId?: string }) {
  const isUser = message.role === "user";
  const isHermes = message.role === "hermes";
  const isOpenClaw = message.role === "openclaw";
  const isSystem = message.role === "system";
  const visibleContent = message.content.replace(/<!--\s*consensus:reached\s*-->/gi, "").trim();
  const timestamp = message.created_at ? new Date(message.created_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "";

  const [rating, setRating] = useState<1 | -1 | 0>(0);
  const [adding, setAdding] = useState(false);
  const [added, setAdded] = useState(false);
  const pushToast = useToastStore((s) => s.pushToast);
  const t = useT();

  async function rate(value: 1 | -1) {
    const next = rating === value ? 0 : value;
    setRating(next);
    if (next !== 0) {
      try {
        await feedbackApi.submit(message.id, next);
      } catch {
        setRating(rating);
      }
    }
  }
  async function addToRoadmap() {
    if (adding || added) return;
    setAdding(true);
    try {
      const parsedList = parseTasksFromMessage(visibleContent);
      let createdCount = 0;
      let firstTitle = "";
      for (const parsed of parsedList) {
        try {
          await tasksApi.create(projectId, { ...parsed, source_message_id: message.id });
          createdCount += 1;
          if (!firstTitle) firstTitle = parsed.title;
        } catch (e) {
          // continue with the rest of the batch even if one fails
          console.warn("Failed to create roadmap task:", e);
        }
      }
      if (createdCount > 0) {
        setAdded(true);
        pushToast({
          tone: "success",
          title: createdCount === 1 ? "Added to Roadmap" : `Added ${createdCount} tasks to Roadmap`,
          description: createdCount === 1 ? firstTitle : `${firstTitle} +${createdCount - 1} more`,
        });
      } else {
        pushToast({ tone: "error", title: "Failed to add to Roadmap", description: "No tasks were created" });
      }
    } catch (e) {
      pushToast({ tone: "error", title: "Failed to add to Roadmap", description: e instanceof Error ? e.message : "" });
    } finally {
      setAdding(false);
    }
  }
  const isStreamingBuffer = message.id.startsWith("streaming-buffer-");
  const canRate = (isHermes || isOpenClaw) && !streaming && !isStreamingBuffer;
  const isDebateRound = message.agent_name?.includes("Round") ?? false;
  const canAddToRoadmap = (isHermes || isOpenClaw) && !streaming && !isStreamingBuffer && !isDebateRound;

  if (isSystem) {
    return (
      <div className="mx-auto flex max-w-[80%] items-start gap-2 rounded-md border border-[#FECACA] bg-[#FEF2F2] px-3 py-2 text-xs text-[#991B1B]">
        <AlertCircle size={13} className="mt-0.5 flex-shrink-0" />
        <span className="whitespace-pre-wrap">{visibleContent}</span>
      </div>
    );
  }

  return (
    <div className={cn("flex gap-3", isUser && "flex-row-reverse justify-start")}>
      <div className={cn(
        "flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full text-xs font-bold text-white",
        isUser && "bg-[#002D62]",
        isHermes && "bg-[#7C3AED]",
        isOpenClaw && "bg-[#0050A0]"
      )}>
        {isUser ? <User size={14} /> : isHermes ? <Bot size={14} /> : <Cpu size={14} />}
      </div>

      <div className={cn(isUser ? "min-w-0 max-w-[38rem] flex flex-col items-end" : "min-w-0 max-w-[56rem]")}>
        <div className={cn("mb-1 flex flex-wrap items-center gap-2", isUser && "justify-end")}>
          {!isUser && (
            <span className={cn(
              "block text-[12px] font-semibold tracking-[-0.01em]",
              isHermes ? "text-[#7C3AED]" : "text-[#0050A0]"
            )}>
              {message.agent_name ?? (isHermes ? "Hermes" : "OpenClaw")}
              {streaming && <span className="ml-1 animate-pulse">●</span>}
            </span>
          )}
          {/* Author label for user-authored messages. With project sharing
              live, a single conversation can carry turns from multiple
              collaborators — show the display name so threads stay
              attributable. Falls back to "You" for the current viewer
              when the joined name is missing (e.g. very fresh INSERTs
              that haven't been re-fetched yet). Typography mirrors the
              agent label above (text-[12px] tracking-[-0.01em]). */}
          {isUser && (() => {
            const isMe = !!currentUserId && message.user_id === currentUserId;
            const label = message.author_name
              ? message.author_name + (isMe ? " (you)" : "")
              : isMe
                ? "You"
                : "User";
            return (
              <span className="block text-[12px] font-semibold tracking-[-0.01em] text-[#002D62]">
                {label}
              </span>
            );
          })()}
          {streaming && <span className="rounded-full border border-[#E2E8F0] bg-white px-2.5 py-1 text-[12px] font-medium text-[#64748B]">Streaming</span>}
          {timestamp && <span className="text-[12px] text-[#94A3B8]">{timestamp}</span>}
        </div>
        <div className={cn(
          "rounded-[28px] px-5 py-4 text-[15px] leading-7 shadow-[0_12px_32px_rgba(15,23,42,0.06)]",
          isUser
            ? "rounded-tr-md bg-[#002D62] text-white"
            : isHermes
              ? "rounded-tl-md border border-[#E9D5FF] bg-[linear-gradient(180deg,#FFFFFF_0%,#F5F3FF_100%)] text-[#1A1A2E]"
              : "rounded-tl-md border border-[#BFDBFE] bg-[linear-gradient(180deg,#FFFFFF_0%,#EFF6FF_100%)] text-[#1A1A2E]"
        )}>
          {isUser || streaming ? (
            <p className="whitespace-pre-wrap text-[15px] leading-7">{visibleContent}</p>
          ) : (
            <div className="prose max-w-none text-[15px] leading-7 prose-headings:text-[#1A1A2E] prose-p:text-[#1A1A2E] prose-p:my-2 prose-li:text-[#334155] prose-li:my-0.5 prose-strong:text-[#0F172A] prose-code:text-[#1E293B] prose-pre:rounded-2xl prose-pre:border prose-pre:border-[#E2E8F0] prose-pre:bg-[#F8FAFC] prose-pre:text-[#0F172A]">
              <ReactMarkdown
                components={{
                  code({ className, children, ...props }) {
                    const match = /language-(\w+)/.exec(className || "");
                    return match ? (
                      <SyntaxHighlighter language={match[1]}>{String(children)}</SyntaxHighlighter>
                    ) : (
                      <code className="rounded bg-black/10 px-1.5 py-0.5 font-mono text-[12px]" {...props}>
                        {children}
                      </code>
                    );
                  },
                }}
              >
                {visibleContent}
              </ReactMarkdown>
            </div>
          )}
        </div>
        {(canRate || canAddToRoadmap) && (
          <div className="mt-2 flex items-center gap-1.5 opacity-60 transition-opacity hover:opacity-100">
            {canRate && (
              <>
                <button
                  onClick={() => void rate(1)}
                  title={t("chat.helpful")}
                  className={cn(
                    "rounded p-1 hover:bg-[#F1F5F9]",
                    rating === 1 && "bg-[#ECFDF5] text-[#10B981]"
                  )}
                >
                  <ThumbsUp size={12} />
                </button>
                <button
                  onClick={() => void rate(-1)}
                  title={t("chat.notHelpful")}
                  className={cn(
                    "rounded p-1 hover:bg-[#F1F5F9]",
                    rating === -1 && "bg-[#FEF2F2] text-[#C8102E]"
                  )}
                >
                  <ThumbsDown size={12} />
                </button>
              </>
            )}
            {canAddToRoadmap && (
              <button
                onClick={() => void addToRoadmap()}
                disabled={adding || added}
                title={added ? t("chat.added") : t("chat.addToRoadmap")}
                className={cn(
                  "ml-1 flex items-center gap-1 rounded px-2.5 py-1.5 text-[12px] hover:bg-[#F1F5F9]",
                  added && "text-[#10B981]"
                )}
              >
                <ClipboardList size={12} /> {added ? t("chat.added") : adding ? t("chat.adding") : t("chat.addToRoadmap")}
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
});

function FileNodeItem({
  node,
  depth,
  selectedPath,
  onSelect,
}: {
  node: FileNode;
  depth: number;
  selectedPath: string;
  onSelect: (path: string) => void;
}) {
  const [open, setOpen] = useState(depth === 0);

  if (node.is_dir) {
    return (
      <div>
        <button
          onClick={() => setOpen((prev) => !prev)}
          className="flex w-full items-center gap-1.5 px-4 py-1.5 text-xs text-[#64748B] hover:bg-[#F8F9FA] hover:text-[#1A1A2E]"
          style={{ paddingLeft: `${16 + depth * 12}px` }}
        >
          {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <FolderOpen size={12} className="text-[#F59E0B]" />
          <span className="truncate">{node.name}</span>
        </button>
        {open && node.children?.map((child) => (
          <FileNodeItem
            key={child.path}
            node={child}
            depth={depth + 1}
            selectedPath={selectedPath}
            onSelect={onSelect}
          />
        ))}
      </div>
    );
  }

  const selected = selectedPath === node.path;
  return (
    <button
      onClick={() => void onSelect(node.path)}
      className={cn(
        "flex w-full items-center gap-1.5 px-4 py-1.5 text-left text-xs transition",
        selected
          ? "bg-[#EAF2FF] text-[#0050A0]"
          : "text-[#94A3B8] hover:bg-[#F8F9FA] hover:text-[#64748B]"
      )}
      style={{ paddingLeft: `${16 + depth * 12 + 16}px` }}
    >
      <File size={11} />
      <span className="truncate">{node.name}</span>
    </button>
  );
}


type ParsedTask = {
  title: string;
  why?: string;
  affected_files?: string[];
  acceptance_criteria?: string;
  estimated_effort?: string;
  priority?: "low" | "medium" | "high" | "critical";
};

const PRIORITY_VALUES = ["low", "medium", "high", "critical"] as const;

function classifyPriority(text: string): ParsedTask["priority"] {
  const lower = text.toLowerCase();
  if (/critical|嚴重|安全漏洞/.test(lower)) return "critical";
  if (/high priority|high\b|高優先|高風險/.test(lower)) return "high";
  if (/low priority|low\b|nice to have|錦上添花/.test(lower)) return "low";
  return "medium";
}

function extractFiles(text: string, limit = 8): string[] {
  const fileRe = /(?:[\w./-]+)\.(?:rs|ts|tsx|js|jsx|py|md|sql|toml|json|yaml|yml|swift|kt|java|go|html|css)\b/g;
  const seen = new Set<string>();
  const out: string[] = [];
  for (const m of text.matchAll(fileRe)) {
    const path = m[0].replace(/^[`"\x27]+|[`"\x27]+$/g, "");
    if (path.length > 120 || path.includes(" ")) continue;
    if (seen.has(path)) continue;
    seen.add(path);
    out.push(path);
    if (out.length >= limit) break;
  }
  return out;
}

function normalizeParsed(raw: Partial<ParsedTask> & { title?: unknown }): ParsedTask | null {
  const titleRaw = typeof raw.title === "string" ? raw.title : "";
  const title = titleRaw
    .replace(/^[#*\-•·\d.\s]+/, "")
    .replace(/[*_`]+/g, "")
    .slice(0, 120)
    .trim();
  if (!title) return null;
  return {
    title,
    why: typeof raw.why === "string" && raw.why.trim() ? raw.why.trim().slice(0, 1500) : undefined,
    affected_files: Array.isArray(raw.affected_files) && raw.affected_files.length > 0
      ? raw.affected_files.map(String).filter(Boolean).slice(0, 12)
      : undefined,
    acceptance_criteria: typeof raw.acceptance_criteria === "string" && raw.acceptance_criteria.trim()
      ? raw.acceptance_criteria.trim().slice(0, 1500)
      : undefined,
    estimated_effort: typeof raw.estimated_effort === "string" && raw.estimated_effort.trim()
      ? raw.estimated_effort.trim().slice(0, 40)
      : undefined,
    priority: PRIORITY_VALUES.includes(raw.priority as never)
      ? (raw.priority as ParsedTask["priority"])
      : "medium",
  };
}

function tryParseJsonTasks(content: string): ParsedTask[] | null {
  // Look for ```json ... ``` or ``` ... ``` fences containing a JSON array of tasks.
  const fenceMatch = content.match(/```(?:json|JSON)?\s*([\s\S]+?)```/);
  const candidates: string[] = [];
  if (fenceMatch) candidates.push(fenceMatch[1]);
  // Bare JSON array fallback (only if message looks like one).
  const trimmed = content.trim();
  if (trimmed.startsWith("[") && trimmed.endsWith("]")) candidates.push(trimmed);
  for (const raw of candidates) {
    try {
      const parsed = JSON.parse(raw.trim());
      if (Array.isArray(parsed)) {
        const tasks = parsed
          .map((item) => (typeof item === "object" && item ? normalizeParsed(item) : null))
          .filter((task): task is ParsedTask => task !== null);
        if (tasks.length > 0) return tasks;
      }
    } catch { /* not valid JSON, fall through */ }
  }
  return null;
}

function tryParseNumberedList(content: string): ParsedTask[] | null {
  // Look for numbered headings like "1) Title" / "1. Title" / "### 1. Title"
  // followed by indented or unindented lines until the next number or end.
  const lines = content.split(/\r?\n/);
  const blocks: { title: string; body: string[] }[] = [];
  const startRe = /^\s*(?:#{1,6}\s+)?(\d+)\s*[).、:：]\s*(.+)$/;
  let current: { title: string; body: string[] } | null = null;
  for (const line of lines) {
    const m = line.match(startRe);
    if (m) {
      if (current && current.title) blocks.push(current);
      current = { title: m[2].trim(), body: [] };
    } else if (current) {
      current.body.push(line);
    }
  }
  if (current && current.title) blocks.push(current);
  if (blocks.length < 2) return null;

  return blocks.map((b) => {
    const body = b.body.join("\n").trim();
    const acMatch = body.match(/(?:acceptance(?:\s*criteria)?|驗收(?:條件)?|verification|tests?)[:：]?\s*([\s\S]+?)(?:\n\s*\n|$)/i);
    const effortMatch = body.match(/(?:effort|工作量|estimated\s*effort)[:：]?\s*([SMLXxlsmh\d./\s大中小]+)/i);
    const filesMatch = body.match(/(?:affected\s*files?|影響檔案|files?)[:：]?\s*([^\n]+)/i);
    let files: string[] | undefined = undefined;
    if (filesMatch) {
      files = filesMatch[1].split(/[,，\s]+/).map((s) => s.replace(/^[`"\x27]+|[`"\x27]+$/g, "")).filter(Boolean);
    }
    if (!files || files.length === 0) {
      const fromBody = extractFiles(body);
      if (fromBody.length > 0) files = fromBody;
    }
    return normalizeParsed({
      title: b.title,
      why: body || undefined,
      affected_files: files,
      acceptance_criteria: acMatch?.[1].trim(),
      estimated_effort: effortMatch?.[1].trim(),
      priority: classifyPriority(b.title + "\n" + body),
    });
  }).filter((task): task is ParsedTask => task !== null);
}

/**
 * Parse one or more roadmap tasks from an agent reply. Tries JSON fence
 * first (most precise), then numbered list (multi-task), and finally
 * falls back to a single-task heuristic so existing behavior is preserved.
 */
function parseTasksFromMessage(content: string): ParsedTask[] {
  const json = tryParseJsonTasks(content);
  if (json && json.length > 0) return json;

  const numbered = tryParseNumberedList(content);
  if (numbered && numbered.length > 1) return numbered;

  // Single-task heuristic (legacy behavior)
  const lines = content.split(/\r?\n/).map((l) => l.trim()).filter((l) => l.length > 0);
  const titleLine = lines[0] ?? "(untitled)";
  const title = titleLine
    .replace(/^[#*\-•·\d.\s]+/, "")
    .replace(/[*_`]+/g, "")
    .slice(0, 100)
    .trim() || "(untitled)";
  const why = lines.slice(1).join("\n").slice(0, 800).trim() || undefined;
  const files = extractFiles(content);
  return [{
    title,
    why,
    affected_files: files.length > 0 ? files : undefined,
    priority: classifyPriority(content),
  }];
}
