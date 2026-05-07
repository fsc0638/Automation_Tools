# Kway In-house Dev Automation Tools

## Project structure
- `backend/`: Rust Axum backend, PostgreSQL via sqlx, reqwest-based agent clients.
- `web/`: Next.js frontend. Read `web/AGENTS.md` before changing frontend code because this Next.js version has breaking changes and local docs in `node_modules/next/dist/docs/` should be consulted.
- `ios/`: iOS client workspace.
- `docker-compose.yml`: local Postgres, backend, and web services.

## Agent connection model
- The project should connect to Hermes through the local Hermes Gateway API Server.
- Backend Hermes endpoint is configured in `backend/.env`:
  - `HERMES_API_URL=http://127.0.0.1:8642/v1`
  - `HERMES_MODEL=hermes-agent`
- Do not point the project directly at Claude Code. Claude Code is used on the assistant/development side; the application talks to Hermes Gateway using the OpenAI-compatible `/v1/chat/completions` API.

## Development notes
- Avoid committing real secrets from `.env` files.
- Prefer `.env.example` for documented placeholder values.
- When changing frontend code, check the installed Next.js docs under `web/node_modules/next/dist/docs/` before relying on older conventions.
