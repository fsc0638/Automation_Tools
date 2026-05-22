# Mac Mini Deployment

## Purpose

The Mac Mini is the private AI Agent runtime host for Kway Dev. It runs the backend and connects to local Hermes / OpenClaw gateways for long-running task execution, Git operations, file processing, and synchronized user workspaces.

The Mac Mini is a compute node, not the owner of user data. User data must remain scoped by user / workspace / project ACL, encrypted workspace boundaries, and active User KEK sessions.

## Recommended topology

```text
User device
  ⇄ HTTPS / WebSocket over Tailscale
Mac Mini
  ├─ Kway Web frontend
  ├─ Kway Backend API
  ├─ PostgreSQL
  ├─ Hermes Gateway API Server
  ├─ OpenClaw Gateway
  └─ Per-user encrypted workspaces
```

OpenClaw should stay loopback-only and be exposed through Tailscale Serve:

```bash
openclaw config set gateway.bind loopback
openclaw config set gateway.tailscale.mode serve
openclaw gateway restart
```

Expected OpenClaw endpoints:

```text
Local:  http://127.0.0.1:18789/
Remote: https://kwayrdcmac-mini.tail315af3.ts.net/
API:    http://127.0.0.1:18789/v1
```

Do not combine `gateway.bind=lan` with `gateway.tailscale.mode=serve`; Serve requires the gateway to bind loopback.

## Backend environment baseline

For a Mac Mini host-run deployment, prefer absolute host paths:

```env
SERVER_HOST=127.0.0.1
SERVER_PORT=8080

PROJECT_DATA_ROOT=/Users/kwayrdc/kway-project-data
DMG_ROOT=/Users/kwayrdc/kway-dmg-store
DMG_SIZE_MB=4096

OPENCLAW_API_URL=http://127.0.0.1:18789/v1
OPENCLAW_MODEL=gpt-5.5

HERMES_API_URL=http://127.0.0.1:8642/v1
HERMES_MODEL=hermes-agent

GIT_TOKEN_ENCRYPTION_KEY=<base64-32-byte-key>
JWT_SECRET=<strong-random-secret>
CORS_ALLOWED_ORIGINS=https://kwayrdcmac-mini.tail315af3.ts.net
```

### Directory layout

```text
/Users/kwayrdc/kway-project-data/
└── users/
    └── <user_id>/                 # mounted encrypted sparse image

/Users/kwayrdc/kway-dmg-store/
└── <user_id>.sparseimage          # APFS encrypted sparse image
```

On login, the backend derives the User KEK, opens the stored per-user DMG passphrase from the vault, and mounts the image at `PROJECT_DATA_ROOT/users/<user_id>/`. On logout, it best-effort detaches the image.

## Docker note

The included `docker-compose.yml` is suitable for local development. It is not the preferred production layout for Mac Mini encrypted user workspaces because macOS APFS sparse image mounting happens on the host, while the backend container writes to container volumes unless explicitly bind-mounted.

For production-like Mac Mini use, prefer either:

1. host-run backend with absolute `PROJECT_DATA_ROOT` / `DMG_ROOT`; or
2. carefully configured bind mounts from the host encrypted workspace paths into the backend container.

Do not assume the default Docker named volume `project_data:/app/data` provides the same security properties as per-user encrypted DMG workspaces.

## Security baseline

- Keep OpenClaw and backend services loopback-only unless protected by Tailscale / reverse proxy.
- Require strong `JWT_SECRET` and `GIT_TOKEN_ENCRYPTION_KEY`.
- Keep user file paths under `PROJECT_DATA_ROOT/users/<user_id>/`.
- Use `user_can_access_project()` before reading project data.
- Require active User KEK for sensitive vault, Git, and encrypted file operations.
- Record audit events for file access, Git operations, and agent context assembly.

## Health checks

```bash
openclaw gateway status
tailscale serve status
curl http://127.0.0.1:8080/healthz
curl http://127.0.0.1:8080/readyz
```

Expected OpenClaw Tailscale Serve output:

```text
https://kwayrdcmac-mini.tail315af3.ts.net (tailnet only)
|-- / proxy http://127.0.0.1:18789
```
