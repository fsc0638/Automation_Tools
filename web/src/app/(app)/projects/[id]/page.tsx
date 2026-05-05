"use client";
import { useEffect, useRef, useState, use } from "react";
import { useRouter } from "next/navigation";
import ReactMarkdown from "react-markdown";
import { SyntaxHighlighter } from "@/components/SyntaxHighlighter";
import {
  MessageSquarePlus, Send, FolderOpen, ChevronRight, ChevronDown,
  GitBranch, Bot, Cpu, User, Zap, ArrowLeft, Plus, File
} from "lucide-react";
import {
  projects as projectsApi, conversations as convsApi,
  createWsConnection, type Project, type Conversation,
  type Message, type FileNode,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { cn, formatDate } from "@/lib/utils";

type AgentMode = "openclaw" | "hermes" | "debate";

export default function ProjectPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = use(params);
  const router = useRouter();

  const [project, setProject] = useState<Project | null>(null);
  const [fileTree, setFileTree] = useState<FileNode[]>([]);
  const [convs, setConvs] = useState<Conversation[]>([]);
  const [activeConv, setActiveConv] = useState<Conversation | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<AgentMode>("openclaw");
  const [streaming, setStreaming] = useState(false);
  const [streamBuffers, setStreamBuffers] = useState<Record<string, string>>({});
  const wsRef = useRef<WebSocket | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    loadProject();
  }, [id]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamBuffers]);

  async function loadProject() {
    const [p, files, cs] = await Promise.all([
      projectsApi.get(id),
      projectsApi.fileTree(id).catch(() => []),
      convsApi.list(id),
    ]);
    setProject(p);
    setFileTree(files);
    setConvs(cs);
    if (cs.length > 0) selectConv(cs[0]);
  }

  async function selectConv(conv: Conversation) {
    setActiveConv(conv);
    const data = await convsApi.get(id, conv.id);
    setMessages(data.messages);
  }

  async function newConv() {
    const conv = await convsApi.create(id, `Conversation ${convs.length + 1}`);
    setConvs((cs) => [conv, ...cs]);
    setMessages([]);
    setActiveConv(conv);
  }

  function sendViaWs(content: string) {
    if (!activeConv) return;
    wsRef.current?.close();
    const ws = createWsConnection(activeConv.id, id);
    wsRef.current = ws;
    setStreaming(true);
    setStreamBuffers({});

    const userMsg: Message = {
      id: Date.now().toString(),
      conversation_id: activeConv.id,
      role: "user",
      content,
      created_at: new Date().toISOString(),
    };
    setMessages((ms) => [...ms, userMsg]);

    ws.onopen = () => {
      ws.send(JSON.stringify({ type: "message", content, mode }));
    };

    ws.onmessage = (e) => {
      const evt = JSON.parse(e.data);
      if (evt.type === "chunk") {
        setStreamBuffers((b) => ({ ...b, [evt.agent]: (b[evt.agent] ?? "") + evt.content }));
      } else if (evt.type === "done") {
        setStreamBuffers((b) => {
          const content = b[evt.agent] ?? "";
          if (content) {
            const role = evt.agent === "Hermes" ? "hermes" : "openclaw";
            setMessages((ms) => [
              ...ms,
              {
                id: Date.now().toString() + evt.agent,
                conversation_id: activeConv.id,
                role,
                content,
                agent_name: evt.agent,
                created_at: new Date().toISOString(),
              } as Message,
            ]);
          }
          const next = { ...b };
          delete next[evt.agent];
          if (Object.keys(next).length === 0) setStreaming(false);
          return next;
        });
      }
    };

    ws.onclose = () => setStreaming(false);
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
            <p className="text-xs font-semibold text-[#94A3B8] uppercase tracking-wider">Conversations</p>
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
              {m === "openclaw" ? "OpenClaw" : m === "hermes" ? "Hermes" : "Debate Mode"}
            </button>
          ))}
          {mode === "debate" && (
            <span className="text-xs text-[#94A3B8] ml-2">Agents will challenge each other</span>
          )}
        </div>

        {/* Messages */}
        <div className="flex-1 overflow-auto px-6 py-6 space-y-4">
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
              {Object.entries(streamBuffers).map(([agent, content]) => (
                content && (
                  <ChatMessage key={`stream-${agent}`} message={{
                    id: `stream-${agent}`, conversation_id: "", role: agent === "Hermes" ? "hermes" : "openclaw",
                    content, agent_name: agent, created_at: new Date().toISOString(),
                  }} streaming />
                )
              ))}
              <div ref={bottomRef} />
            </>
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
            <Button type="submit" disabled={!activeConv || !input.trim() || streaming} loading={streaming}>
              <Send size={15} />
            </Button>
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
            <p className="whitespace-pre-wrap">{message.content}</p>
          ) : (
            <ReactMarkdown
              className="prose prose-sm max-w-none"
              components={{
                code({ node, className, children, ...props }) {
                  const match = /language-(\w+)/.exec(className || "");
                  const isBlock = !!match;
                  return isBlock ? (
                    <SyntaxHighlighter language={match[1]}>{String(children)}</SyntaxHighlighter>
                  ) : (
                    <code className="bg-black/10 rounded px-1 py-0.5 font-mono text-xs" {...props}>
                      {children}
                    </code>
                  );
                },
              }}
            >
              {message.content}
            </ReactMarkdown>
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
