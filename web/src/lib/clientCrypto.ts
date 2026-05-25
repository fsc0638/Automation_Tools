"use client";

// Client-held KEK protocol (Option B, see backend/migrations/0049_client_held_kek.sql).
//
// The user's plaintext password NEVER leaves this device. Instead the
// browser runs Argon2id twice — domain-separated by a constant prefix —
// to produce:
//
//   auth_hash = Argon2id("kway-auth-v1::" || password, kek_salt)  ──► sent to server, proves identity
//   user_kek  = Argon2id("kway-kek-v1::"  || password, kek_salt)  ──► sent to server only on successful login,
//                                                                     held in session_keys for vault ops
//
// Both are 32 raw bytes (base64 over the wire).  The salt + Argon2
// parameters come from GET /auth/kek-params — the server is the single
// source of truth for the contract.
//
// hash-wasm runs Argon2id inside WebAssembly (≈ 500-1500 ms with the
// 64 MiB / 3-iter parameters; acceptable for once-per-login).

import { argon2id } from "hash-wasm";

// ── Wire types ───────────────────────────────────────────────────────

export interface KekParams {
  /** base64 of the 32-byte salt — fed straight into Argon2id. */
  kek_salt: string;
  argon2_m_cost: number;   // KiB
  argon2_t_cost: number;
  argon2_p_cost: number;
  argon2_output_len: number;
  /** Domain prefix prepended to the password before hashing for KEK. */
  kek_domain: string;
  /** Domain prefix prepended to the password before hashing for auth. */
  auth_domain: string;
}

// ── Helpers ──────────────────────────────────────────────────────────

const TEXT = new TextEncoder();

export function base64Encode(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s);
}

export function base64Decode(b64: string): Uint8Array {
  const bin = atob(b64.trim());
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function concatBytes(...parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let off = 0;
  for (const p of parts) { out.set(p, off); off += p.length; }
  return out;
}

// ── Argon2id wrapper ─────────────────────────────────────────────────

async function deriveWithDomain(
  domain: string,
  password: string,
  params: KekParams,
): Promise<Uint8Array> {
  const input = concatBytes(TEXT.encode(domain), TEXT.encode(password));
  const salt = base64Decode(params.kek_salt);
  const result = await argon2id({
    password: input,
    salt,
    parallelism: params.argon2_p_cost,
    iterations: params.argon2_t_cost,
    memorySize: params.argon2_m_cost,
    hashLength: params.argon2_output_len,
    outputType: "binary",
  });
  return result as Uint8Array;
}

/** Run both derivations in sequence and return them base64-encoded. */
export async function deriveAuthAndKek(
  password: string,
  params: KekParams,
): Promise<{ authHash: string; userKek: string; userKekBytes: Uint8Array }> {
  const auth = await deriveWithDomain(params.auth_domain, password, params);
  const kek = await deriveWithDomain(params.kek_domain, password, params);
  return {
    authHash: base64Encode(auth),
    userKek: base64Encode(kek),
    userKekBytes: kek,
  };
}

// ── kek-params fetch ─────────────────────────────────────────────────

const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:8080/api";

export async function fetchKekParams(email: string): Promise<KekParams> {
  const url = `${API_BASE}/auth/kek-params?email=${encodeURIComponent(email)}`;
  const res = await fetch(url, { method: "GET" });
  if (!res.ok) {
    throw new Error("Failed to load login parameters; please retry");
  }
  return res.json();
}

/** Generate a fresh 32-byte salt for new account registration. */
export function randomKekSalt(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return base64Encode(bytes);
}

// ── In-memory KEK holder (UX-only) ───────────────────────────────────
//
// Used to keep the User KEK available across the SPA while the user
// is still on the same tab — purely for "soft-refresh" UX. It is NOT
// load-bearing for security; the server's `session_keys` store is the
// canonical KEK home. We deliberately do NOT persist this anywhere
// (no localStorage, no IndexedDB) — a tab close or hard reload drops
// the value and the user must log in again to repopulate the server.

let inMemoryUserKek: Uint8Array | null = null;

export function rememberUserKek(bytes: Uint8Array): void {
  inMemoryUserKek = new Uint8Array(bytes); // copy
}

export function getRememberedUserKek(): string | null {
  return inMemoryUserKek ? base64Encode(inMemoryUserKek) : null;
}

export function forgetUserKek(): void {
  if (inMemoryUserKek) {
    inMemoryUserKek.fill(0);
    inMemoryUserKek = null;
  }
}
