import type { NextConfig } from "next";
import path from "node:path";

// Pin Turbopack's workspace root to this `web/` directory.
//
// Without this, Turbopack walks upward from the project looking for any
// package.json / package-lock.json to infer the workspace root. If
// there's a stray `package.json` higher up (e.g. C:\Users\<user>\package.json
// from an ad-hoc `npm install playwright` at home), Turbopack picks THAT
// as the root and the module resolver then searches the wrong
// node_modules tree — symptom is "Can't resolve 'tailwindcss'" even
// though it's installed inside web/node_modules.
//
// path.resolve(__dirname) gives us the absolute path of `web/` regardless
// of where `npm run dev` was invoked from. See
// node_modules/next/dist/docs/01-app/03-api-reference/05-config/01-next-config-js/turbopack.md
// for the official documentation of `turbopack.root`.
const nextConfig: NextConfig = {
  allowedDevOrigins: ["127.0.0.1"],
  turbopack: {
    root: path.resolve(__dirname),
  },
};

export default nextConfig;
