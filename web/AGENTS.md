<!-- BEGIN:nextjs-agent-rules -->
# This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` before writing any code. Heed deprecation notices.
<!-- END:nextjs-agent-rules -->

## Dev Console Noise: Browser Extension Warnings

Two recurring red messages in DevTools that are **not code bugs**:

### 1. `fdprocessedid` hydration mismatch

Warning: Prop fdprocessedid did not match. Server: null, Client: "..."

**Source**: A browser extension (Edge Autofill, Form Filler, etc.) injects this attribute into `<input>` / `<button>` elements after SSR but before React hydration. `suppressHydrationWarning` is already applied on `LocaleSwitcher`, `<input>`, and `<button>`. If a new element starts emitting this, add `suppressHydrationWarning` to that element.
**To confirm it's not real**: open an incognito window (extensions disabled) — warning disappears.

### 2. `chrome.runtime` async listener

Uncaught (in promise) Error: A listener indicated an asynchronous response
by returning true, but the message channel closed before a response was received

**Source**: Same family — a content script returned `true` from `chrome.runtime.onMessage` (declared async) but its `sendResponse` callback was never called before the page navigated or unmounted. This originates in the extension, not our code. No fix needed on our side.
**Rule of thumb**: if either message appears in a normal window but disappears in incognito → extension noise, move on.
