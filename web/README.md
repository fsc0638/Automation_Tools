This is a [Next.js](https://nextjs.org) project bootstrapped with [`create-next-app`](https://nextjs.org/docs/app/api-reference/cli/create-next-app).

## Getting Started

First, run the development server:

```bash
npm run dev
# or
yarn dev
# or
pnpm dev
# or
bun dev
```

Open [http://localhost:3000](http://localhost:3000) with your browser to see the result.

You can start editing the page by modifying `app/page.tsx`. The page auto-updates as you edit the file.

This project uses [`next/font`](https://nextjs.org/docs/app/building-your-application/optimizing/fonts) to automatically optimize and load [Geist](https://vercel.com/font), a new font family for Vercel.

## Dev console noise — what to ignore

Some console warnings appear during development that are NOT bugs in this app:

- **`Unchecked runtime.lastError: The message port closed before a response was received.`** — emitted by Chrome browser extensions (LastPass, Grammarly, MetaMask, etc.) trying to talk to their own background page. Has nothing to do with our WebSocket or fetch calls. Reproduces in a clean profile only if an extension is installed; disappears in Incognito with extensions disabled.
- **`Hydration failed because the server rendered HTML didn't match the client. fdprocessedid="..."`** — injected by password managers / form-fill extensions onto `<input>` and `<button>` between SSR and client hydration. Suppressed at the component level via `suppressHydrationWarning`; if you still see it, the offending element is a raw `<input>` or `<button>` rather than the `ui/input.tsx` / `ui/button.tsx` wrappers — switch it over.
- **`WebSocket is closed before the connection is established.`** — React 18+ StrictMode double-mounts effects in dev. The second mount's cleanup fires before the WS handshake completes. Handled by `safeCloseWs()` in `projects/[id]/page.tsx`; if you see this in production it's a real bug.

## Learn More

To learn more about Next.js, take a look at the following resources:

- [Next.js Documentation](https://nextjs.org/docs) - learn about Next.js features and API.
- [Learn Next.js](https://nextjs.org/learn) - an interactive Next.js tutorial.

You can check out [the Next.js GitHub repository](https://github.com/vercel/next.js) - your feedback and contributions are welcome!

## Deploy on Vercel

The easiest way to deploy your Next.js app is to use the [Vercel Platform](https://vercel.com/new?utm_medium=default-template&filter=next.js&utm_source=create-next-app&utm_campaign=create-next-app-readme) from the creators of Next.js.

Check out our [Next.js deployment documentation](https://nextjs.org/docs/app/building-your-application/deploying) for more details.
