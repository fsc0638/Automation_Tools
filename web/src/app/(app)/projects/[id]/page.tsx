"use client";
import { useCallback, useEffect, useRef, useState, use } from "react";
import { useRouter } from "next/navigation";
import ReactMarkdown from "react-markdown";
import { SyntaxHighlighter } from "@/components/SyntaxHighlighter";
import {
  MessageSquarePlus, Send, FolderOpen, ChevronRight, ChevronDown,
  Bot, Cpu, User, Zap, ArrowLeft, Plus, File, GitBranch, Square, AlertCircle, ArrowDown
} from "lucide-react";
import {
  projects as projectsApi, conversations as convsApi,
  createWsConnection, type Project, type Conversation,
  type Message, type FileNode, type AgentMode,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

const MODE_LABELS: Record<AgentMode, string> = {
  openclaw: "OpenClaw",
  hermes: "Hermes",
  debate: "Debate Mode",
};

function displayAgentName(agent: string, round?: number, phase?: string) {
  if (phase === "round" && round) return `${agent} · Round ${round}`;
  if (phase === "final") return `${agent} · Final`;
  return agent;
}

export default function ProjectPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = use(params);
  const router = useRouter();

  const [project, setProject] = useState<Project | null>(null);
  const [fileTree, setFileTree] = useState<FileNode[]>([]);
  const [branches, setBranches] = useState<string[]>([]);
  const [switchingBranch, setSwitchingBranch] = useState(false);
  const [convs, setConvs] = useState<Conversation[]>([]);
  const [activeConv, setActiveConv] = useState<Conversation | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<AgentMode>("openclaw");
  const [streaming, setStreaming] = useState(false);
  const [streamBuffers, setStreamBuffers] = useState<Record<string, string>>({});
  const [showJumpToBottom, setShowJumpToBottom] = useState(false);
  const streamBuffersRef = useRef<Record<string, string>>({});
  const wsRef = useRef<WebSocket | null>(null);
  const [wsReconnectKey, setWsReconnectKey] = useState(0);
  const messagesScrollRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const shouldAutoScrollRef = useRef(true);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const selectConv = useCallback(async (conv: Conversation) => {
    setActiveConv(conv);
    const data = await convsApi.get(id, conv.id);
    setMessages(data.messages);
  }, [id]);

  // Effect 1: project + file tree + branches — reload only when project id changes.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const p = await projectsApi.get(id);
      const [files, branchData] = await Promise.all([
        projectsApi.fileTree(id).catch(() => []),
        p.source_type === "git"
          ? projectsApi.gitBranches(id).catch(() => ({ branches: [] }))
          : Promise.resolve({ branches: [] }),
      ]);
      if (cancelled) return;
      setProject(p);
      setFileTree(files);
      setBranches(branchData.branches);
    })();
    return () => { cancelled = true; };
  }, [id]);

  // Effect 2: conversations list — reload when project id or mode changes.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const cs = await convsApi.list(id, mode);
      if (cancelled) return;
      setConvs(cs);
      streamBuffersRef.current = {};
      setStreamBuffers({});
      setStreaming(false);
      if (cs.length > 0) {
        setActiveConv(cs[0]);
        const data = await convsApi.get(id, cs[0].id);
        if (!cancelled) setMessages(data.messages);
      } else {
        setActiveConv(null);
        setMessages([]);
      }
    })();
    return () => { cancelled = true; };
  }, [id, mode]);

  // Effect 3: WebSocket bound to activeConv lifecycle.
  // Opens once per conversation; sendViaWs reuses the live connection.
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
        setStreamBuffers({});
        setStreaming(false);
        return;
      }

      if (!evt.agent) return;
      const label = displayAgentName(evt.agent, evt.round, evt.phase);
      if (evt.type === "chunk" && evt.content) {
        streamBuffersRef.current[label] = (streamBuffersRef.current[label] ?? "") + evt.content;
        setStreamBuffers({ ...streamBuffersRef.current });
        if (!shouldAutoScrollRef.current) setShowJumpToBottom(true);
      } else if (evt.type === "done") {
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
        setStreamBuffers({ ...streamBuffersRef.current });
        if (Object.keys(streamBuffersRef.current).length === 0) setStreaming(false);
      }
    };

    ws.onclose = () => {
      if (wsRef.current === ws) {
        setStreaming(false);
      }
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
      setStreaming(false);
    };

    return () => {
      ws.close();
      if (wsRef.current === ws) wsRef.current = null;
    };
  }, [activeConv?.id, id, wsReconnectKey]);

  useEffect(() => {
    if (!shouldAutoScrollRef.current) return;
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamBuffers]);

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
  }

  function sendViaWs(content: string) {
    if (!activeConv) return;
    const ws = wsRef.current;
    const convId = activeConv.id;

    shouldAutoScrollRef.current = true;
    setShowJumpToBottom(false);
    streamBuffersRef.current = {};
    setStreamBuffers({});
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
      // Connection lost — trigger reconnect, then send when ready.
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
    setStreamBuffers({});
    setStreaming(false);
    // Force the WS effect to reconnect for the next message.
    setWsReconnectKey((k) => k + 1);
  }

  async function switchBranch(branch: string) {
    if (!project || project.source_type !== "git" || !branch || branch === project.default_branch) return;
    setSwitchingBranch(true);
    try {
      const updated = await projectsApi.checkoutBranch(id, branch);
      const [files, branchData] = await Promise.all([
        projectsApi.fileTree(id).catch(() => []),
        projectsApi.gitBranches(id).catch(() => ({ branches: [] })),
      ]);
      setProject(updated);
      setFileTree(files);
      setBranches(branchData.branches);
      streamBuffersRef.current = {};
      setStreamBuffers({});
    } finally {
      setSwitchingBranch(false);
    }
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!input.trim() || streaming) return;
    sendViaWs(input.trim());
    setInput("");
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e as unknown as React.FormEvent);
    }
  }

  return (
    <div className="flex h-full">
      {/* Left: File Tree + Conversations */}
      <div className="w-64 flex-shrink-0 bg-white border-r border-[#E2E8F0] flex flex-col">
        {/* Back + Project name */}
        <div className="px-4 py-3 border-b border-[#E2E8F0]">
          <button onClick={() => router.push("/projects")}
            className="flex items-center gap-1.5 text-xs text-[#64748B] hover:text-[#1A1A2E] mb-2">
            <ArrowLeft size={12} /> Projects
          </button>
          <h2 className="font-semibold text-[#1A1A2E] text-sm truncate">{project?.name}</h2>
        </div>

        {/* File Tree */}
        <div className="flex-1 overflow-auto">
          <div className="px-4 py-2 border-b border-[#F1F5F9]">
            <p className="text-xs font-semibold text-[#94A3B8] uppercase tracking-wider">Files</p>
          </div>
          <div className="py-1">
            {fileTree.map((node) => <FileNodeItem key={node.path} node={node} depth={0} />)}
          </div>

          <div className="px-4 py-2 border-t border-[#F1F5F9] flex items-center justify-between">
            <p className="text-xs font-semibold text-[#94A3B8] uppercase tracking-wider">{MODE_LABELS[mode]} Chats</p>
            <button onClick={newConv} className="text-[#94A3B8] hover:text-[#0050A0]">
              <Plus size={13} />
            </button>
          </div>
          {convs.map((c) => (
            <button key={c.id}
              onClick={() => selectConv(c)}
              className={cn(
                "w-full text-left px-4 py-2 text-sm transition-colors",
                activeConv?.id === c.id
                  ? "bg-blue-50 text-[#0050A0] font-medium"
                  : "text-[#64748B] hover:bg-[#F8F9FA]"
              )}>
              <div className="flex items-center gap-2">
                <MessageSquarePlus size={13} />
                <span className="truncate">{c.title}</span>
              </div>
            </button>
          ))}
        </div>
      </div>

      {/* Right: Chat */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Toolbar */}
        <div className="h-14 border-b border-[#E2E8F0] bg-white flex items-center px-6 gap-4">
          {project?.source_type === "git" && (
            <div className="flex items-center gap-2 border-r border-[#E2E8F0] pr-4 mr-1">
              <GitBranch size={13} className="text-[#0050A0]" />
              <select
                value={project.default_branch ?? ""}
                disabled={switchingBranch || streaming}
                onChange={(e) => void switchBranch(e.target.value)}
                className="h-8 rounded-md border border-[#E2E8F0] bg-white px-2 text-xs text-[#1A1A2E] disabled:opacity-50"
                title="Switch Git branch"
              >
                {(branches.length ? branches : [project.default_branch ?? "main"]).map((branch) => (
                  <option key={branch} value={branch}>{branch}</option>
                ))}
              </select>
              {switchingBranch && <span className="text-xs text-[#94A3B8]">switching...</span>}
            </div>
          )}
          <span className="text-sm font-medium text-[#64748B]">Mode:</span>
          {(["openclaw", "hermes", "debate"] as AgentMode[]).map((m) => (
            <button key={m}
              onClick={() => setMode(m)}
              className={cn(
                "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors",
                mode === m
                  ? "bg-[#0050A0] text-white"
                  : "text-[#64748B] hover:bg-[#F1F5F9]"
              )}>
              {m === "openclaw" && <Cpu size={12} />}
              {m === "hermes" && <Bot size={12} />}
              {m === "debate" && <Zap size={12} />}
              {MODE_LABELS[m] as string}
            </button>
          ))}
          {mode === "debate" && (
            <span className="text-xs text-[#94A3B8] ml-2">Agents will challenge each other</span>
          )}
        </div>

        {/* Messages */}
        <div
          ref={messagesScrollRef}
          onScroll={handleMessagesScroll}
          className="relative flex-1 overflow-auto px-6 py-6 space-y-4"
        >
          {!activeConv ? (
            <div className="flex flex-col items-center justify-center h-full text-center">
              <MessageSquarePlus size={48} className="text-[#E2E8F0] mb-4" />
              <p className="text-[#64748B] font-medium">No conversation selected</p>
              <p className="text-[#94A3B8] text-sm mt-1">Create a new conversation to start</p>
              <Button className="mt-4" onClick={newConv}><Plus size={14} /> New Conversation</Button>
            </div>
          ) : (
            <>
              {messages.map((msg) => <ChatMessage key={msg.id} message={msg} />)}

              {/* Streaming buffers */}
              {Object.entries(streamBuffers).map(([agentLabel, content]) =>
                content ? (
                  <ChatMessage key={`streaming-buffer-${agentLabel}`} message={{
                    id: `streaming-buffer-${agentLabel}`, conversation_id: "", role: agentLabel.startsWith("Hermes") ? "hermes" : "openclaw",
                    content, agent_name: agentLabel, created_at: new Date().toISOString(),
                  }} streaming />
                ) : null
              )}
              <div ref={bottomRef} />
            </>
          )}
          {showJumpToBottom && (
            <button
              type="button"
              onClick={jumpToBottom}
              className="sticky bottom-3 left-1/2 -translate-x-1/2 flex items-center gap-1.5 rounded-full border border-[#BFDBFE] bg-white px-3 py-1.5 text-xs font-medium text-[#0050A0] shadow-sm hover:bg-blue-50"
            >
              <ArrowDown size={13} /> New output
            </button>
          )}
        </div>

        {/* Input */}
        <div className="border-t border-[#E2E8F0] bg-white px-6 py-4">
          <form onSubmit={handleSubmit} className="flex gap-3 items-end">
            <textarea
              ref={textareaRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={activeConv ? "Ask the agents... (Enter to send, Shift+Enter for newline)" : "Select a conversation first"}
              disabled={!activeConv || streaming}
              rows={1}
              className={cn(
                "flex-1 resize-none rounded-lg border border-[#E2E8F0] px-4 py-2.5 text-sm text-[#1A1A2E]",
                "placeholder:text-[#94A3B8] focus:outline-none focus:ring-2 focus:ring-[#0050A0] focus:border-transparent",
                "disabled:opacity-50 disabled:cursor-not-allowed min-h-[42px] max-h-40 overflow-auto"
              )}
              style={{ height: "auto" }}
              onInput={(e) => {
                const t = e.currentTarget;
                t.style.height = "auto";
                t.style.height = Math.min(t.scrollHeight, 160) + "px";
              }}
            />
            {streaming ? (
              <Button type="button" variant="secondary" onClick={stopStreaming}>
                <Square size={14} /> Stop
              </Button>
            ) : (
              <Button type="submit" disabled={!activeConv || !input.trim()}>
                <Send size={15} />
              </Button>
            )}
          </form>
        </div>
      </div>
    </div>
  );
}

function ChatMessage({ message, streaming }: { message: Message; streaming?: boolean }) {
  const isUser = message.role === "user";
  const isHermes = message.role === "hermes";
  const isOpenClaw = message.role === "openclaw";
  const isSystem = message.role === "system";
  const visibleContent = message.content.replace(/<!--\s*consensus:reached\s*-->/gi, "").trim();

  if (isSystem) {
    return (
      <div className="flex items-start gap-2 mx-auto max-w-[80%] rounded-md border border-[#FECACA] bg-[#FEF2F2] px-3 py-2 text-xs text-[#991B1B]">
        <AlertCircle size={13} className="flex-shrink-0 mt-0.5" />
        <span className="whitespace-pre-wrap">{visibleContent}</span>
      </div>
    );
  }

  return (
    <div className={cn("flex gap-3", isUser && "flex-row-reverse")}>
      {/* Avatar */}
      <div className={cn(
        "w-8 h-8 rounded-full flex items-center justify-center flex-shrink-0 text-white text-xs font-bold",
        isUser && "bg-[#002D62]",
        isHermes && "bg-[#7C3AED]",
        isOpenClaw && "bg-[#0050A0]",
      )}>
        {isUser ? <User size={14} /> : isHermes ? <Bot size={14} /> : <Cpu size={14} />}
      </div>

      {/* Bubble */}
      <div className={cn("max-w-[75%]", isUser && "items-end flex flex-col")}>
        {!isUser && (
          <span className={cn(
            "text-xs font-semibold mb-1 block",
            isHermes ? "text-[#7C3AED]" : "text-[#0050A0]"
          )}>
            {message.agent_name ?? (isHermes ? "Hermes" : "OpenClaw")}
            {streaming && <span className="ml-1 animate-pulse">●</span>}
          </span>
        )}
        <div className={cn(
          "rounded-xl px-4 py-3 text-sm",
          isUser
            ? "bg-[#002D62] text-white rounded-tr-sm"
            : isHermes
              ? "bg-[#F5F3FF] border border-[#E9D5FF] text-[#1A1A2E] rounded-tl-sm"
              : "bg-[#EFF6FF] border border-[#BFDBFE] text-[#1A1A2E] rounded-tl-sm"
        )}>
          {isUser ? (
            <p className="whitespace-pre-wrap">{visibleContent}</p>
          ) : (
            <div className="prose prose-sm max-w-none">
              <ReactMarkdown
                components={{
                  code({ className, children, ...props }) {
                    const match = /language-(\w+)/.exec(className || "");
                    return match ? (
                      <SyntaxHighlighter language={match[1]}>{String(children)}</SyntaxHighlighter>
                    ) : (
                      <code className="bg-black/10 rounded px-1 py-0.5 font-mono text-xs" {...props}>
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
      </div>
    </div>
  );
}

function FileNodeItem({ node, depth }: { node: FileNode; depth: number }) {
  const [open, setOpen] = useState(depth === 0);
  if (node.is_dir) {
    return (
      <div>
        <button
          onClick={() => setOpen((o) => !o)}
          className="w-full flex items-center gap-1.5 px-4 py-1 text-xs text-[#64748B] hover:bg-[#F8F9FA] hover:text-[#1A1A2E]"
          style={{ paddingLeft: `${16 + depth * 12}px` }}>
          {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <FolderOpen size={12} className="text-[#F59E0B]" />
          {node.name}
        </button>
        {open && node.children?.map((child) => (
          <FileNodeItem key={child.path} node={child} depth={depth + 1} />
        ))}
      </div>
    );
  }
  return (
    <div className="flex items-center gap-1.5 px-4 py-1 text-xs text-[#94A3B8] hover:bg-[#F8F9FA] hover:text-[#64748B] cursor-pointer"
      style={{ paddingLeft: `${16 + depth * 12 + 16}px` }}>
      <File size={11} />
      {node.name}
    </div>
  );
}
