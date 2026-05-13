"""Render the 8 meeting architecture diagrams as PNGs using graphviz.

Run from repo root:  python3 scripts/render_meeting_diagrams.py
Output:              assets/meeting-diagrams/*.png
"""

import os
from graphviz import Digraph

OUTPUT_DIR = "assets/meeting-diagrams"
FONT = "PingFang TC"  # macOS Traditional Chinese font with good CJK coverage

os.makedirs(OUTPUT_DIR, exist_ok=True)


def base(name: str, title: str, rankdir: str = "LR") -> Digraph:
    g = Digraph(name, format="png")
    g.attr(
        rankdir=rankdir,
        fontname=FONT,
        fontsize="14",
        labelloc="t",
        label=title,
        bgcolor="white",
        dpi="160",
        pad="0.4",
        nodesep="0.35",
        ranksep="0.5",
    )
    g.attr(
        "node",
        shape="box",
        style="rounded,filled",
        fillcolor="#f5f5f5",
        fontname=FONT,
        fontsize="11",
        margin="0.12,0.08",
    )
    g.attr("edge", fontname=FONT, fontsize="10", color="#666666")
    return g


def save(g: Digraph, filename: str) -> None:
    g.render(filename=f"{OUTPUT_DIR}/{filename}", cleanup=True)
    print(f"  ✓ {OUTPUT_DIR}/{filename}.png")


# ---------------------------------------------------------------------------
# 1. System overview
# ---------------------------------------------------------------------------
def d1_system_overview():
    g = base("d1", "圖 1：系統總覽（誰跟誰講話）", rankdir="LR")

    with g.subgraph(name="cluster_client") as c:
        c.attr(label="Client", style="filled,rounded", fillcolor="#e7f3ff", fontname=FONT)
        c.node("web", "Web\\n(Next.js)")
        c.node("ios", "iOS App")

    with g.subgraph(name="cluster_backend") as c:
        c.attr(label="Backend (Rust Axum)", style="filled,rounded", fillcolor="#fff4e6", fontname=FONT)
        c.node("api", "REST + WebSocket API")
        c.node("orch", "Agent Orchestrator", fillcolor="#ffe8cc")
        c.node("pol", "Agent Data Policy 檢查")
        c.node("fw", "Context Firewall\\n+ DLP 脫敏", fillcolor="#fde2e4")
        c.edge("api", "orch")
        c.edge("orch", "pol")
        c.edge("pol", "fw")

    with g.subgraph(name="cluster_db") as c:
        c.attr(label="PostgreSQL", style="filled,rounded", fillcolor="#e3f9e5", fontname=FONT)
        c.node("mem", "四層記憶\\n對話/專案/便利貼/程式碼索引", shape="cylinder", fillcolor="#d4edda")
        c.node("acl", "組織/工作區/Project ACL", shape="cylinder", fillcolor="#d4edda")
        c.node("audit", "Audit Log\\n(只存 hash + 阻擋清單)", shape="cylinder", fillcolor="#cfe2ff")

    with g.subgraph(name="cluster_agents") as c:
        c.attr(label="外部 Agent Gateway", style="filled,rounded", fillcolor="#f3e5f5", fontname=FONT)
        c.node("oc", "OpenClaw")
        c.node("her", "Hermes")
        c.node("cus", "用戶自訂 LLM")

    g.edge("web", "api", label="JWT")
    g.edge("ios", "api", label="JWT")
    g.edge("orch", "mem", dir="both")
    g.edge("orch", "acl", label="權限檢查", style="dashed")
    g.edge("fw", "audit")
    g.edge("fw", "oc")
    g.edge("fw", "her")
    g.edge("fw", "cus")

    save(g, "01-system-overview")


# ---------------------------------------------------------------------------
# 2. Memory layers — write & read
# ---------------------------------------------------------------------------
def d2_memory_layers():
    g = base("d2", "圖 2：記憶四層 — 寫入與讀取", rankdir="TB")

    with g.subgraph(name="cluster_write") as c:
        c.attr(label="寫入路徑", style="filled,rounded", fillcolor="#fff4e6", fontname=FONT)
        c.node("u1", "用戶送訊息")
        c.node("msg", "messages", shape="cylinder", fillcolor="#d4edda")
        c.node("auto", "每輪自動")
        c.node("cs", "conversation_summaries\\n(對話摘要)", shape="cylinder", fillcolor="#d4edda")
        c.node("batch", "累積到門檻")
        c.node("llm", "LLM 生成候選")
        c.node("cand", "memory_candidates\\n(pending)", shape="cylinder", fillcolor="#fff3cd")
        c.node("pms", "project_memory_summaries\\n(專案記憶)", shape="cylinder", fillcolor="#d4edda")
        c.node("u2", "用戶手動建")
        c.node("note", "shared_memory_notes\\n(共用便利貼)", shape="cylinder", fillcolor="#d4edda")
        c.node("u3", "匯入 / 上傳程式碼")
        c.node("files", "project_files + chunks\\n(程式碼索引)", shape="cylinder", fillcolor="#d4edda")

        c.edge("u1", "msg")
        c.edge("msg", "auto")
        c.edge("auto", "cs")
        c.edge("msg", "batch")
        c.edge("batch", "llm")
        c.edge("llm", "cand")
        c.edge("cand", "pms", label="人類核可", color="#28a745", fontcolor="#28a745")
        c.edge("u2", "note")
        c.edge("u3", "files")

    with g.subgraph(name="cluster_read") as c:
        c.attr(
            label="讀取路徑（每次 Agent 呼叫前）",
            style="filled,rounded",
            fillcolor="#e7f3ff",
            fontname=FONT,
        )
        c.node("ctx", "Orchestrator 組 Context", fillcolor="#cfe2ff")
        c.node("r1", "最近對話歷史")
        c.node("r2", "該專案記憶摘要")
        c.node("r3", "相關程式碼片段")
        c.node("send", "送 Agent", fillcolor="#d4edda")
        c.edge("ctx", "r1")
        c.edge("ctx", "r2")
        c.edge("ctx", "r3")
        c.edge("r1", "send")
        c.edge("r2", "send")
        c.edge("r3", "send")

    g.edge("msg", "r1", style="dashed", constraint="false")
    g.edge("pms", "r2", style="dashed", constraint="false")
    g.edge("files", "r3", style="dashed", constraint="false")
    g.edge("note", "ctx", label="目前未自動注入\\n需手動引用", style="dashed", color="#d9534f", fontcolor="#d9534f")

    save(g, "02-memory-layers")


# ---------------------------------------------------------------------------
# 3. Learning A — Memory approval flow
# ---------------------------------------------------------------------------
def d3_memory_approval():
    g = base("d3", "圖 3：學習模式 A — 記憶審核流程", rankdir="LR")

    g.node("t", "每輪對話結束")
    g.node("q1", "訊息數達門檻?", shape="diamond", fillcolor="#fff3cd")
    g.node("end1", "僅更新對話摘要")
    g.node("gen", "LLM 生成新版專案摘要")
    g.node("hash", "計算\\nsource_context_hash")
    g.node("q2", "已有相同 pending?", shape="diamond", fillcolor="#fff3cd")
    g.node("skip", "略過避免堆積")
    g.node("ins", "memory_candidates\\nstatus = pending", shape="cylinder", fillcolor="#fff3cd")
    g.node("rev", "用戶在 UI 審核", shape="diamond", fillcolor="#cfe2ff")
    g.node("write", "覆寫\\nproject_memory_summaries", fillcolor="#d4edda")
    g.node("note", "留審核註解\\nstatus = rejected", fillcolor="#f8d7da")
    g.node("stamp", "記錄 reviewed_by\\n+ applied_at", fillcolor="#d4edda")

    g.edge("t", "q1")
    g.edge("q1", "end1", label="否")
    g.edge("q1", "gen", label="是")
    g.edge("gen", "hash")
    g.edge("hash", "q2")
    g.edge("q2", "skip", label="是")
    g.edge("q2", "ins", label="否")
    g.edge("ins", "rev")
    g.edge("rev", "write", label="Approve", color="#28a745", fontcolor="#28a745")
    g.edge("rev", "note", label="Reject", color="#d9534f", fontcolor="#d9534f")
    g.edge("write", "stamp")

    save(g, "03-memory-approval-flow")


# ---------------------------------------------------------------------------
# 4. Learning B — Message feedback
# ---------------------------------------------------------------------------
def d4_feedback():
    g = base("d4", "圖 4：學習模式 B — 訊息評分（目前只進 Dashboard）", rankdir="LR")

    g.node("msg", "Agent 回應")
    g.node("thumb", "用戶按 👍 / 👎\\n可加註解")
    g.node("fb", "message_feedback\\n(每用戶每訊息 1 票)", shape="cylinder", fillcolor="#d4edda")
    g.node("dash", "Insights Dashboard", fillcolor="#cfe2ff")
    g.node("m1", "每 Agent 滿意率")
    g.node("m2", "每 Agent 票數")

    g.node("x1", "Agent 路由調整", style="dashed,filled", fillcolor="#fff3cd")
    g.node("x2", "Prompt 自動優化", style="dashed,filled", fillcolor="#fff3cd")
    g.node("x3", "低分回答淘汰", style="dashed,filled", fillcolor="#fff3cd")

    g.edge("msg", "thumb")
    g.edge("thumb", "fb")
    g.edge("fb", "dash")
    g.edge("dash", "m1")
    g.edge("dash", "m2")
    g.edge("fb", "x1", style="dashed", label="尚未閉環", color="#aaaaaa", fontcolor="#aaaaaa")
    g.edge("fb", "x2", style="dashed", color="#aaaaaa")
    g.edge("fb", "x3", style="dashed", color="#aaaaaa")

    save(g, "04-feedback-loop")


# ---------------------------------------------------------------------------
# 5. Permission three-layer check
# ---------------------------------------------------------------------------
def d5_permissions():
    g = base("d5", "圖 5：權限三層判定（SQL user_can_access_project）", rankdir="TB")

    g.node("req", "請求：使用者 U 存取 Project P\\n所需最低角色 = R", fillcolor="#fff4e6")
    g.node("check", "以下任一條件成立?", shape="diamond", fillcolor="#cfe2ff")
    g.node("a", "U 是 P 的建立者")
    g.node("b", "U 在 project_acl\\n角色權重 ≥ R")
    g.node("c", "U 在 P 的 Workspace\\n角色權重 ≥ R")
    g.node("d", "U 在 P 的 Organization\\n角色權重 ≥ R")
    g.node("ok", "允許", fillcolor="#d4edda")
    g.node("deny", "拒絕", fillcolor="#f8d7da")

    with g.subgraph(name="cluster_rank") as c:
        c.attr(label="角色權重", style="filled,rounded", fillcolor="#f5f5f5", fontname=FONT)
        c.node("r1", "owner = 40", fillcolor="#ffffff")
        c.node("r2", "admin = 30", fillcolor="#ffffff")
        c.node("r3", "editor / member = 20", fillcolor="#ffffff")
        c.node("r4", "viewer = 10", fillcolor="#ffffff")

    g.edge("req", "check")
    g.edge("check", "a")
    g.edge("check", "b")
    g.edge("check", "c")
    g.edge("check", "d")
    g.edge("a", "ok", label="至少一條 YES", color="#28a745", fontcolor="#28a745")
    g.edge("b", "ok", color="#28a745")
    g.edge("c", "ok", color="#28a745")
    g.edge("d", "ok", color="#28a745")
    g.edge("check", "deny", label="全 NO", color="#d9534f", fontcolor="#d9534f", style="dashed")

    save(g, "05-permission-check")


# ---------------------------------------------------------------------------
# 6. Context Firewall pipeline
# ---------------------------------------------------------------------------
def d6_context_firewall():
    g = base("d6", "圖 6：Context Firewall — 每次呼叫 Agent 都會跑", rankdir="TB")

    g.node("in", "原始 Context\\n程式碼 + 對話歷史 + 摘要", fillcolor="#fff4e6")
    g.node("p", "讀取 Agent 的 Data Policy", fillcolor="#cfe2ff")

    g.node("q0", "external_processing\\n_allowed?", shape="diamond", fillcolor="#fff3cd")
    g.node("allblk", "全部擋下", fillcolor="#f8d7da")

    g.node("q1", "allow_code_context?", shape="diamond", fillcolor="#fff3cd")
    g.node("q2", "allow_project_memory?", shape="diamond", fillcolor="#fff3cd")
    g.node("q3", "allow_conversation\\n_history?", shape="diamond", fillcolor="#fff3cd")

    g.node("red", "Regex 脫敏\\nAPI key / token → REDACTED", fillcolor="#fff3cd")
    g.node("cls", "依檔案路徑分類\\npublic → secret")
    g.node("q4", "分類 >\\nallowed_classification_max?", shape="diamond", fillcolor="#fff3cd")
    g.node("bhi", "該段內容擋下", fillcolor="#f8d7da")
    g.node("keep", "保留", fillcolor="#d4edda")

    g.node("out", "組合最終 Context\\n計算 SHA256 hash", fillcolor="#d4edda")
    g.node("send", "送 Agent", fillcolor="#d4edda")
    g.node("aud", "audit log\\n只存 hash + 阻擋清單\\n不存原文", shape="cylinder", fillcolor="#cfe2ff")

    g.edge("in", "p")
    g.edge("p", "q0")
    g.edge("q0", "allblk", label="否", color="#d9534f", fontcolor="#d9534f")
    g.edge("q0", "q1", label="是", color="#28a745", fontcolor="#28a745")
    g.edge("q1", "q2")
    g.edge("q2", "q3")
    g.edge("q3", "red")
    g.edge("red", "cls")
    g.edge("cls", "q4")
    g.edge("q4", "bhi", label="是", color="#d9534f", fontcolor="#d9534f")
    g.edge("q4", "keep", label="否", color="#28a745", fontcolor="#28a745")
    g.edge("bhi", "out")
    g.edge("keep", "out")
    g.edge("out", "send")
    g.edge("out", "aud")

    save(g, "06-context-firewall")


# ---------------------------------------------------------------------------
# 7. Cross-account memory linking
# ---------------------------------------------------------------------------
def d7_cross_account():
    g = base("d7", "圖 7：跨帳號連動 — 三條路徑", rankdir="TB")

    with g.subgraph(name="cluster_a") as c:
        c.attr(label="路徑 A：加進「專案」(最常用)", style="filled,rounded",
               fillcolor="#e3f9e5", fontname=FONT)
        c.node("ua1", "User A", fillcolor="#cfe2ff")
        c.node("p1", "Project P", fillcolor="#ffe8cc")
        c.node("ub1", "User B", fillcolor="#cfe2ff")
        c.node("shared1", "B 可看：\\n對話 / 訊息 / 專案記憶 / 程式碼索引", fillcolor="#d4edda")
        c.edge("ua1", "p1", label="擁有")
        c.edge("ua1", "p1", label="把 B 加入 project_acl\\n角色 = editor",
               color="#28a745", fontcolor="#28a745")
        c.edge("ub1", "p1", label="透過 ACL 取得", style="dashed")
        c.edge("p1", "shared1")

    with g.subgraph(name="cluster_b") as c:
        c.attr(label="路徑 B：加進「組織 / 工作區」(團隊規模)", style="filled,rounded",
               fillcolor="#fff4e6", fontname=FONT)
        c.node("org", "Organization", fillcolor="#ffe8cc")
        c.node("ws", "Workspace", fillcolor="#ffe8cc")
        c.node("p2", "Project X")
        c.node("p3", "Project Y")
        c.node("p4", "Project Z")
        c.node("ub2", "User B 加進 Workspace", fillcolor="#cfe2ff")
        c.edge("org", "ws")
        c.edge("ws", "p2")
        c.edge("ws", "p3")
        c.edge("ws", "p4")
        c.edge("ub2", "ws")
        c.edge("ub2", "p2", label="自動取得", style="dashed", color="#28a745", fontcolor="#28a745")
        c.edge("ub2", "p3", style="dashed", color="#28a745")
        c.edge("ub2", "p4", style="dashed", color="#28a745")

    with g.subgraph(name="cluster_c") as c:
        c.attr(label="路徑 C：共用便利貼 (目前不跨帳號)", style="filled,rounded",
               fillcolor="#fde2e4", fontname=FONT)
        c.node("ua3", "User A", fillcolor="#cfe2ff")
        c.node("note", "shared_memory_notes\\n綁 user_id", shape="cylinder", fillcolor="#f8d7da")
        c.node("ub3", "User B\\n即使共享專案", fillcolor="#cfe2ff")
        c.edge("ua3", "note", label="寫入")
        c.edge("ub3", "note", label="看不到 ❌", color="#d9534f", fontcolor="#d9534f", style="dashed")

    save(g, "07-cross-account-linking")


# ---------------------------------------------------------------------------
# 8. Done vs Gap vs Future
# ---------------------------------------------------------------------------
def d8_done_gap_future():
    g = base("d8", "圖 8：現況 vs 落差 vs 未來方向", rankdir="LR")

    with g.subgraph(name="cluster_done") as c:
        c.attr(label="已落地", style="filled,rounded", fillcolor="#d4edda", fontname=FONT)
        c.node("d1", "三層權限 ACL")
        c.node("d2", "Context Firewall + DLP")
        c.node("d3", "Audit Log")
        c.node("d4", "Git Token AES-256 加密")
        c.node("d5", "專案記憶人類審核")
        c.node("d6", "Agent 細粒度資料政策")
        c.node("d7", "訊息評分 + Dashboard")

    with g.subgraph(name="cluster_gap") as c:
        c.attr(label="落差", style="filled,rounded", fillcolor="#fff3cd", fontname=FONT)
        c.node("g1", "共用便利貼\\n未自動注入 agent")
        c.node("g2", "評分尚未閉環\\n不影響 agent 行為")
        c.node("g3", "共用便利貼\\n不跨帳號")

    with g.subgraph(name="cluster_future") as c:
        c.attr(label="可決策方向", style="filled,rounded", fillcolor="#cfe2ff", fontname=FONT)
        c.node("f1", "記憶向量化\\n取代詞彙索引?")
        c.node("f2", "組織級共用記憶?")
        c.node("f3", "評分閉環\\n低分自動降權?")

    g.edge("g1", "f2", style="dashed", color="#888888")
    g.edge("g3", "f2", style="dashed", color="#888888")
    g.edge("g2", "f3", style="dashed", color="#888888")

    save(g, "08-done-gap-future")


if __name__ == "__main__":
    print(f"Rendering diagrams to {OUTPUT_DIR}/ ...")
    d1_system_overview()
    d2_memory_layers()
    d3_memory_approval()
    d4_feedback()
    d5_permissions()
    d6_context_firewall()
    d7_cross_account()
    d8_done_gap_future()
    print("Done.")
