# Role mapping  —  AgentK ↔ Kway Dev

Kway Dev's existing permission ladder is the canonical naming in this folder. Memory-weight policy stays unchanged.

| Authority | AgentK | Kway Dev | Storage |
| --- | --- | --- | --- |
| Highest (workspace-wide) | admin | **owner** | `workspace_members.role` |
| Per-meeting primary | owner | **admin** | `meeting_participants.role='admin'` |
| Per-meeting deputy | delegate | **editor** | `meeting_participants.role='editor'` |
| Per-meeting member | attendee | **viewer** | `meeting_participants.role='viewer'` |

Reading any AgentK doc, substitute mentally:

    AgentK 'admin'    → Kway Dev 'owner'
    AgentK 'owner'    → Kway Dev 'admin'
    AgentK 'delegate' → Kway Dev 'editor'
    AgentK 'attendee' → Kway Dev 'viewer'

When porting AgentK code, rename the string literals on the way in. The semantic ladder is identical; only the labels swap.