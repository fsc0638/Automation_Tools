"use client";
import { memo, use, useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { useRouter } from "next/navigation";
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
  feedback as feedbackApi,
  projects as projectsApi,
  tasks as tasksApi,
  type AgentMode,
  type Conversation,
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
import { useWorkspaceChromeStore } from "@/lib/store";
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
      "請把目前專案可優化方向整理成可執行 Roadmap。請輸出任務清單，每個任務包含：title、priority、why、affected files、acceptance criteria、estimated effort、dependencies、建議由 OpenClaw 或 Hermes 主導。任務必須根據專案檔案與目前對話，不要憑空發明。",
  },
  {
    key: "patch",
    labelKey: "quick.patchPlan",
    icon: Code2,
    prompt:
      "請進入 Patch / PR 規劃模式。根據目前專案狀態，挑選最高價值且風險可控的一項改善，產生 patch-ready 計畫。請輸出：目標、受影響檔案、修改步驟、預期 diff 摘要、測試指令、回滾方案、PR 標題與描述。不要實際 commit 或 push；若證據不足，先列出需要讀取或確認的檔案。",
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
  const pushToast = useToastStore((state) => state.pushToast);
  const t = useT();
  const modeLabel = (m: AgentMode): string =>
    m === "openclaw" ? t("chat.modeOpenClaw")
    : m === "hermes" ? t("chat.modeHermes")
    : t("chat.modeDebate");
  const setShowAppSidebar = useWorkspaceChromeStore((state) => state.setShowAppSidebar);

  const [project, setProject] = useState<Project | null>(null);
  const [fileTree, setFileTree] = useState<FileNode[]>([]);
  const [branches, setBranches] = useState<string[]>([]);
  const [gitStatus, setGitStatus] = useState<GitStatus | null>(null);
  const [switchingBranch, setSwitchingBranch] = useState(false);
  const [convs, setConvs] = useState<Conversation[]>([]);
  const [activeConv, setActiveConv] = useState<Conversation | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<AgentMode>("openclaw");
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
  const [projectTab, setProjectTab] = useState<ProjectTab>("workspace");
  const [fileQuery, setFileQuery] = useState("");
  const [selectedFilePath, setSelectedFilePath] = useState("");
  const [selectedFileContent, setSelectedFileContent] = useState("");
  const [filePreviewLoading, setFilePreviewLoading] = useState(false);
  const [filePreviewError, setFilePreviewError] = useState("");
  const [showConversationRail, setShowConversationRail] = useState(true);
  const [showContextRail, setShowContextRail] = useState(false);
  const [showWorkspaceOverview, setShowWorkspaceOverview] = useState(false);
  const [showConversationSummary, setShowConversationSummary] = useState(false);
  const [showDebateWorkflow, setShowDebateWorkflow] = useState(false);
  const [showComposerTools, setShowComposerTools] = useState(false);
  const [focusMode, setFocusMode] = useState(false);

  const streamBuffersRef = useRef<Record<string, string>>({});
  const streamStatusesRef = useRef<Record<string, StreamStatus>>({});
  const wsRef = useRef<WebSocket | null>(null);
  const [wsReconnectKey, setWsReconnectKey] = useState(0);
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
        title: "OpenClaw proposes",
        detail: "Fast first pass and implementation angle.",
        state: hasOpenClaw ? (hasHermes || hasFinal ? "done" : "active") : mode === "debate" && streaming ? "active" : "idle",
      },
      {
        title: "Hermes challenges",
        detail: "Counterpoints, risks, and stronger reasoning.",
        state: hasHermes ? (hasFinal ? "done" : "active") : mode === "debate" && (hasOpenClaw || streaming) ? "queued" : "idle",
      },
      {
        title: "Final synthesis",
        detail: "Unified recommendation with trade-offs resolved.",
        state: hasFinal ? "active" : mode === "debate" && (hasOpenClaw || hasHermes || streaming) ? "queued" : "idle",
      },
    ] as const;
  }, [mode, streamStatusEntries, streaming]);

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
      const [files, branchData, status] = await Promise.all([
        projectsApi.fileTree(id).catch(() => []),
        p.source_type === "git"
          ? projectsApi.gitBranches(id).catch(() => ({ branches: [] }))
          : Promise.resolve({ branches: [] }),
        p.source_type === "git"
          ? projectsApi.gitStatus(id).catch(() => null)
          : Promise.resolve(null),
      ]);
      if (cancelled) return;
      setProject(p);
      setFileTree(files);
      setBranches(branchData.branches);
      setGitStatus(status);
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

  useEffect(() => {
    if (!activeConv) {
      wsRef.current?.close();
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
      ws.close();
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
      const behavior: ScrollBehavior = streaming ? "auto" : "smooth";
      bottomRef.current?.scrollIntoView({ behavior });
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
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }

  async function newConv() {
    const conv = await convsApi.create(id, `${MODE_LABELS[mode]} Conversation ${convs.length + 1}`, mode);
    setConvs((cs) => [conv, ...cs]);
    setMessages([]);
    setActiveConv(conv);
    setMode(conv.mode);
    pushToast({ tone: "success", title: "Conversation created", description: `${MODE_LABELS[conv.mode]} is ready for the next turn.` });
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
    await convsApi.delete(id, conv.id);
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
      pushToast({ tone: "success", title: "Branch switched", description: `Workspace is now on ${branch}.` });
      setTimeout(() => setRefreshStatus(""), 3000);
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Branch switch failed";
      setRefreshStatus(`Switch failed: ${msg}`);
      pushToast({ tone: "error", title: "Branch switch failed", description: msg });
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
                <h1 className="text-xl font-semibold text-[#1A1A2E]">{project?.name ?? t("project.workspace")}</h1>
                {project && <StatusPill>{project.source_type === "git" ? t("project.gitRepository") : project.source_type === "upload" ? t("project.uploadProject") : t("project.localFolder")}</StatusPill>}
                <StatusPill className={MODE_STYLES[mode]}>{modeLabel(mode)}</StatusPill>
                {focusMode && <StatusPill className="bg-[#EAF2FF] text-[#0050A0]">{t("chat.focusMode")}</StatusPill>}
              </div>
              <p className="mt-1 text-sm text-[#64748B]">
                {t("project.repoSummary")}
              </p>
              <div className="mt-3 flex flex-wrap items-center gap-2 text-xs text-[#64748B]">
                <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2.5 py-1">Branch: {project?.default_branch ?? "—"}</span>
                <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2.5 py-1">Conversations: {convs.length}</span>
                <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2.5 py-1">Dirty files: {dirtyCount}</span>
                {selectedFilePath && <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2.5 py-1">Focused file: {selectedFilePath.split("/").pop() ?? selectedFilePath}</span>}
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
            <OverviewCard label={t("project.mode")} value={modeLabel(mode)} icon={<Sparkles size={14} />} />
            <OverviewCard label={t("project.selectedFile")} value={selectedFilePath ? selectedFilePath.split("/").pop() ?? selectedFilePath : "—"} icon={<FolderOpen size={14} />} />
            <OverviewCard label={t("project.lastUpdate")} value={project ? formatDate(project.updated_at) : "—"} icon={<Clock3 size={14} />} />
          </div>
        )}

        {refreshStatus && (
          <div className="mt-3">
            <InlineBanner
              tone={refreshStatus.toLowerCase().includes("failed") ? "error" : "info"}
              title="Workspace status"
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
      {projectTab === "roadmap" && <RoadmapTab projectId={id} />}

      {projectTab === "workspace" && (
      <div className="flex min-h-0 flex-1 bg-[#F8FAFC]">
        {showConversationRail && !focusMode && (
        <aside className="w-[280px] flex-shrink-0 border-r border-[#E2E8F0] bg-white">
          <div className="border-b border-[#E2E8F0] px-4 py-4">
            <div className="flex items-start justify-between gap-3">
              <div>
                <p className="text-xs font-semibold uppercase tracking-[0.14em] text-[#94A3B8]">{t("convList.title")}</p>
                <p className="mt-1 text-sm text-[#64748B]">{t("convList.subtitle")}</p>
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
                className="w-full bg-transparent text-sm text-[#1A1A2E] outline-none placeholder:text-[#94A3B8]"
              />
            </div>
          </div>

          <div className="h-[calc(100%-113px)] overflow-auto px-2 py-2">
            {!project ? (
              <div className="space-y-2 px-2 py-2">
                <SkeletonBlock className="h-[88px] w-full" />
                <SkeletonBlock className="h-[88px] w-full" />
                <SkeletonBlock className="h-[88px] w-full" />
              </div>
            ) : filteredConvs.length === 0 ? (
              <SectionEmpty
                className="px-4 py-8"
                title={conversationQuery ? "No conversations found" : "No conversations yet"}
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
                        <span className="truncate text-sm font-medium text-[#1A1A2E]">{conv.title}</span>
                        {activeConv?.id === conv.id && <span className="rounded-full bg-white/90 px-2 py-0.5 text-[11px] font-semibold text-[#0050A0]">{t("convList.active")}</span>}
                      </div>
                      <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-[#64748B]">
                        <span className={cn("rounded-full px-2 py-0.5", MODE_STYLES[conv.mode])}>{MODE_LABELS[conv.mode]}</span>
                        <span>{formatRelativeTime(conv.updated_at)}</span>
                        {streaming && activeConv?.id === conv.id && <span className="rounded-full border border-[#BFDBFE] bg-white px-2 py-0.5 text-[#1D4ED8]">Live</span>}
                      </div>
                      <div className="mt-2 line-clamp-2 text-xs text-[#64748B]">
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
                    <button
                      type="button"
                      onClick={(e) => void deleteConv(conv, e)}
                      title="Delete conversation"
                      className="opacity-0 transition group-hover:opacity-100 text-[#94A3B8] hover:text-[#C8102E]"
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                </div>
              ))
            )}
          </div>
        </aside>
        )}

        <main className="flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="border-b border-[#E2E8F0] bg-white/85 px-5 py-3 backdrop-blur-sm">
            <div className="mx-auto flex w-full max-w-5xl flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
              <div className="min-w-0 space-y-3">
                <div>
                  <div className="flex flex-wrap items-center gap-2">
                    {!showConversationRail && !focusMode && (
                      <Button variant="secondary" size="sm" onClick={() => setShowConversationRail(true)}>
                        <ChevronRight size={14} /> {t("convList.title")}
                      </Button>
                    )}
                    <h2 className="truncate text-sm font-semibold text-[#1A1A2E]">{activeConv?.title ?? t("chat.placeholderEmpty")}</h2>
                    {activeConv && <StatusPill className={MODE_STYLES[activeConv.mode]}>{modeLabel(activeConv.mode)}</StatusPill>}
                    {streaming && <StatusPill className="bg-[#EFF6FF] text-[#1D4ED8]">{t("chat.streaming")}</StatusPill>}
                  </div>
                  <p className="mt-1 text-xs text-[#64748B]">
                    {activeConv ? t("chat.chooseStrategy") : t("convList.emptyDesc")}
                  </p>
                </div>

                {activeConv && (
                  <div className="flex flex-wrap items-center gap-2 text-xs text-[#64748B]">
                    <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2.5 py-1">Thread: {activeThreadSummary.messageCount} messages</span>
                    <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2.5 py-1">Last agent: {activeThreadSummary.lastAgent ?? "Waiting for first reply"}</span>
                    <span className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2.5 py-1">Last user: {activeThreadSummary.lastUserAt ? formatRelativeTime(activeThreadSummary.lastUserAt) : "Not yet"}</span>
                    <button
                      type="button"
                      onClick={() => setShowConversationSummary((value) => !value)}
                      className="rounded-full border border-[#E2E8F0] bg-white px-2.5 py-1 text-[#475569] transition hover:border-[#94A3B8] hover:text-[#1A1A2E]"
                    >
                      {showConversationSummary ? "Hide details" : "Show details"}
                    </button>
                  </div>
                )}

                {showConversationSummary && activeConv && (
                  <div className="grid gap-2 sm:grid-cols-3">
                    <div className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE] px-3 py-2">
                      <div className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">Thread size</div>
                      <div className="mt-1 text-sm font-semibold text-[#1A1A2E]">{activeThreadSummary.messageCount} messages</div>
                    </div>
                    <div className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE] px-3 py-2">
                      <div className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">Last agent</div>
                      <div className="mt-1 truncate text-sm font-semibold text-[#1A1A2E]">{activeThreadSummary.lastAgent ?? "Waiting for first reply"}</div>
                    </div>
                    <div className="rounded-2xl border border-[#E2E8F0] bg-[#FBFCFE] px-3 py-2">
                      <div className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[#94A3B8]">Last user turn</div>
                      <div className="mt-1 text-sm font-semibold text-[#1A1A2E]">{activeThreadSummary.lastUserAt ? formatRelativeTime(activeThreadSummary.lastUserAt) : "Not yet"}</div>
                    </div>
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
              <div className="mt-4 rounded-3xl border border-[#FDE68A] bg-[linear-gradient(180deg,#FFFDF5_0%,#FFFBEB_100%)] p-4 text-sm text-[#92400E] shadow-sm">
                <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                  <div>
                    <div className="font-semibold text-[#92400E]">Debate workflow</div>
                    <p className="mt-1 text-xs text-[#A16207]">Structured disagreement first, synthesis second. Use this when trade-offs or correctness matter more than speed.</p>
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
            className="relative flex-1 overflow-auto px-4 py-6 sm:px-5"
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
              <div className="mx-auto w-full max-w-5xl space-y-5">
                {streaming && (
                  <div className="rounded-3xl border border-[#DBEAFE] bg-[linear-gradient(180deg,#FFFFFF_0%,#F8FBFF_42%,#EFF6FF_100%)] px-5 py-4 shadow-[0_16px_40px_rgba(59,130,246,0.08)]">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div>
                        <div className="text-sm font-semibold text-[#1D4ED8]">Live agent activity</div>
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
                    style={{ contentVisibility: "auto", containIntrinsicSize: "0 200px" }}
                  >
                    <ChatMessage message={msg} projectId={id} />
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
                className="sticky bottom-3 left-1/2 -translate-x-1/2 rounded-full border border-[#BFDBFE] bg-white px-3 py-1.5 text-xs font-medium text-[#0050A0] shadow-sm hover:bg-blue-50"
              >
                <span className="inline-flex items-center gap-1.5">
                  <ArrowDown size={13} /> New output
                </span>
              </button>
            )}
          </div>

          <div className="border-t border-[#E2E8F0] bg-white/92 px-5 py-3 shadow-[0_-10px_30px_rgba(15,23,42,0.04)] backdrop-blur-sm">
            <form onSubmit={handleSubmit} className="mx-auto w-full max-w-5xl space-y-3">
              <div className="flex flex-wrap items-center gap-2 text-xs text-[#64748B]">
                <span className={cn("rounded-full px-2.5 py-1", MODE_STYLES[mode])}>{MODE_LABELS[mode]}</span>
                {selectedFilePath ? (
                  <button
                    type="button"
                    onClick={() => {
                      setSelectedFilePath("");
                      setSelectedFileContent("");
                    }}
                    className="rounded-full border border-[#E2E8F0] bg-[#F8FAFC] px-2.5 py-1 text-[#475569] transition hover:border-[#94A3B8]"
                  >
                    Focused file: {focusedFileName} ×
                  </button>
                ) : (
                  <span className="rounded-full border border-dashed border-[#CBD5E1] bg-white px-2.5 py-1 text-[#94A3B8]">{t("chat.noFocusedFile")}</span>
                )}
                {streaming && <span className="rounded-full border border-[#BFDBFE] bg-[#EFF6FF] px-2.5 py-1 text-[#1D4ED8]">Agents are responding…</span>}
                <button
                  type="button"
                  onClick={() => setShowComposerTools((value) => !value)}
                  className="rounded-full border border-[#E2E8F0] bg-white px-2.5 py-1 text-[#475569] transition hover:border-[#94A3B8] hover:text-[#1A1A2E]"
                >
                  {showComposerTools ? "Hide suggestions" : "Show suggestions"}
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
                        className="rounded-full border border-[#E2E8F0] bg-white px-3 py-1.5 text-xs font-medium text-[#475569] transition hover:border-[#0050A0] hover:text-[#0050A0] disabled:cursor-not-allowed disabled:opacity-50"
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
                      className="rounded-full border border-[#E2E8F0] bg-white px-3 py-1.5 text-xs font-medium text-[#475569] transition hover:border-[#0050A0] hover:text-[#0050A0] disabled:cursor-not-allowed disabled:opacity-50"
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
                      "min-h-[76px] max-h-44 flex-1 resize-none overflow-auto rounded-[24px] border border-[#D6DFEA] bg-white px-4 py-3.5 text-[15px] leading-7 text-[#1A1A2E]",
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
                    <div className="text-[11px] text-[#94A3B8]">{input.trim().length} {t("chat.chars")}</div>
                  </div>
                </div>
              </div>
            </form>
          </div>
        </main>

        {showContextRail && !focusMode && (
        <aside className="w-[320px] flex-shrink-0 border-l border-[#E2E8F0] bg-white xl:flex xl:flex-col">
          <div className="border-b border-[#E2E8F0] px-4 py-4">
            <div className="flex items-center justify-between gap-3">
              <div>
                <p className="text-xs font-semibold uppercase tracking-[0.14em] text-[#94A3B8]">{t("context.title")}</p>
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
                      className="w-full bg-transparent text-sm text-[#1A1A2E] outline-none placeholder:text-[#94A3B8]"
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
    </div>
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
    ? "Done"
    : state === "active"
      ? "Running"
      : state === "queued"
        ? "Queued"
        : "Waiting";

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
            "block text-xs font-semibold",
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

const ChatMessage = memo(function ChatMessage({ message, projectId, streaming }: { message: Message; projectId: string; streaming?: boolean }) {
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
      const parsed = parseTaskFromMessage(visibleContent);
      await tasksApi.create(projectId, { ...parsed, source_message_id: message.id });
      setAdded(true);
      pushToast({ tone: "success", title: "Added to Roadmap", description: parsed.title });
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
    <div className={cn("flex gap-3", isUser && "flex-row-reverse")}>
      <div className={cn(
        "flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full text-xs font-bold text-white",
        isUser && "bg-[#002D62]",
        isHermes && "bg-[#7C3AED]",
        isOpenClaw && "bg-[#0050A0]"
      )}>
        {isUser ? <User size={14} /> : isHermes ? <Bot size={14} /> : <Cpu size={14} />}
      </div>

      <div className={cn(isUser ? "min-w-0 max-w-[42rem] flex flex-col items-end" : "min-w-0 max-w-[52rem]")}>
        <div className={cn("mb-1 flex flex-wrap items-center gap-2", isUser && "justify-end")}>
          {!isUser && (
            <span className={cn(
              "block text-xs font-semibold",
              isHermes ? "text-[#7C3AED]" : "text-[#0050A0]"
            )}>
              {message.agent_name ?? (isHermes ? "Hermes" : "OpenClaw")}
              {streaming && <span className="ml-1 animate-pulse">●</span>}
            </span>
          )}
          {streaming && <span className="rounded-full border border-[#E2E8F0] bg-white px-2 py-0.5 text-[11px] font-medium text-[#64748B]">Streaming</span>}
          {timestamp && <span className="text-[11px] text-[#94A3B8]">{timestamp}</span>}
        </div>
        <div className={cn(
          "rounded-[24px] px-5 py-4 text-[15px] leading-7 shadow-[0_10px_30px_rgba(15,23,42,0.05)]",
          isUser
            ? "rounded-tr-md bg-[#002D62] text-white"
            : isHermes
              ? "rounded-tl-md border border-[#E9D5FF] bg-[linear-gradient(180deg,#FFFFFF_0%,#F5F3FF_100%)] text-[#1A1A2E]"
              : "rounded-tl-md border border-[#BFDBFE] bg-[linear-gradient(180deg,#FFFFFF_0%,#EFF6FF_100%)] text-[#1A1A2E]"
        )}>
          {isUser || streaming ? (
            <p className="whitespace-pre-wrap">{visibleContent}</p>
          ) : (
            <div className="prose prose-sm max-w-none leading-7 prose-headings:text-[#1A1A2E] prose-p:text-[#1A1A2E] prose-li:text-[#334155] prose-strong:text-[#0F172A] prose-code:text-[#1E293B] prose-pre:rounded-2xl prose-pre:border prose-pre:border-[#E2E8F0] prose-pre:bg-[#F8FAFC] prose-pre:text-[#0F172A]">
              <ReactMarkdown
                components={{
                  code({ className, children, ...props }) {
                    const match = /language-(\w+)/.exec(className || "");
                    return match ? (
                      <SyntaxHighlighter language={match[1]}>{String(children)}</SyntaxHighlighter>
                    ) : (
                      <code className="rounded bg-black/10 px-1 py-0.5 font-mono text-xs" {...props}>
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
          <div className="mt-1.5 flex items-center gap-1 opacity-50 transition-opacity hover:opacity-100">
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
                  "ml-1 flex items-center gap-1 rounded px-2 py-1 text-[11px] hover:bg-[#F1F5F9]",
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


/**
 * Pull a roadmap task from an agent reply. Heuristic — title is the first
 * non-empty line, why is the rest, affected_files are regex-extracted code paths.
 */
function parseTaskFromMessage(content: string): {
  title: string;
  why?: string;
  affected_files?: string[];
  priority?: "low" | "medium" | "high" | "critical";
} {
  const lines = content
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0);

  const titleLine = lines[0] ?? "(untitled)";
  const title = titleLine
    .replace(/^[#*\-•·\d.\s]+/, "")
    .replace(/[*_`]+/g, "")
    .slice(0, 100)
    .trim() || "(untitled)";

  const why = lines.slice(1).join("\n").slice(0, 800).trim() || undefined;

  const fileRe = /(?:[\w./-]+)\.(?:rs|ts|tsx|js|jsx|py|md|sql|toml|json|yaml|yml|swift|kt|java|go|html|css)\b/g;
  const seen = new Set<string>();
  const affected_files: string[] = [];
  for (const m of content.matchAll(fileRe)) {
    const path = m[0].replace(/^[`"\x27]+|[`"\x27]+$/g, "");
    if (path.length > 120 || path.includes(" ")) continue;
    if (seen.has(path)) continue;
    seen.add(path);
    affected_files.push(path);
    if (affected_files.length >= 8) break;
  }

  const lower = content.toLowerCase();
  const priority: "low" | "medium" | "high" | "critical" = lower.match(/critical|嚴重|安全漏洞/)
    ? "critical"
    : lower.match(/high priority|high\s|高優先|高風險/)
    ? "high"
    : lower.match(/low priority|low\s|nice to have|錦上添花/)
    ? "low"
    : "medium";

  return {
    title,
    why,
    affected_files: affected_files.length > 0 ? affected_files : undefined,
    priority,
  };
}
