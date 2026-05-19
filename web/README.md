# ContextNest dashboard

The clickable surface for the seven-tool memory API. v0.2-ui scaffold —
see [`../docs/roadmap/v0.2-ui-dashboard.md`](../docs/roadmap/v0.2-ui-dashboard.md)
for the design rationale.

## Dev loop

Prerequisites: Node 22+ and pnpm 9+.

```bash
cd web
cp .env.example .env       # one-time
pnpm install
pnpm dev                   # starts the dashboard on :5173
```

In a second terminal, run the substrate so the dashboard has something
to talk to:

```bash
# from the repo root
cargo run -- serve          # starts the substrate on :8080
```

Open <http://localhost:5173>. The Vite dev server proxies `/api/*` and
`/health` to the substrate, so there's no CORS surgery required.

## Scripts

| Script | What it does |
|---|---|
| `pnpm dev` | Dev server with HMR + proxy to the substrate |
| `pnpm build` | Type-check + production build to `dist/` |
| `pnpm preview` | Serve the built `dist/` for smoke-checking the prod bundle |
| `pnpm typecheck` | `tsc --noEmit` only (faster than full build for CI) |
| `pnpm lint` | ESLint over the source tree |
| `pnpm format` | Prettier auto-format |

## Stack

| Layer | Library | Why |
|---|---|---|
| Build | Vite 6 + React 19 + TypeScript 5.7+ | Fastest dev loop |
| Styling | Tailwind v4 (via `@tailwindcss/vite`) | CSS-first config; no PostCSS chain |
| Components | shadcn-style primitives in `src/components/ui/` | Owned in-tree (MIT), not npm-installed |
| Routing | TanStack Router (code-based) | Type-safe, small footprint |
| Data | TanStack Query v5 | Stale-while-revalidate; perfect for the seven-tool API |
| Client state | Zustand v5 | Lightweight; no Redux drama |
| Icons | lucide-react | Standard icon set; tree-shakeable |

## Layout

```
web/src/
├── main.tsx               # entry — wires QueryClient + Router
├── App.tsx                # root component
├── index.css              # Tailwind v4 import + theme tokens
├── lib/
│   ├── api.ts             # typed client for the seven-tool API
│   ├── cn.ts              # clsx + tailwind-merge helper
│   └── query-client.ts    # TanStack Query setup
├── components/
│   ├── layout/
│   │   ├── shell.tsx      # outer chrome (sidebar + main)
│   │   └── sidebar.tsx    # left-rail navigation
│   └── ui/
│       ├── button.tsx
│       ├── card.tsx
│       └── input.tsx
└── routes/
    ├── providers.tsx      # working — provider catalog
    ├── playground.tsx     # stub — seven-tool forms (phase 2)
    └── memories.tsx       # stub — session memory list (phase 3)
```

## What lands when

- **Phase 1 (this PR):** scaffold + working Providers screen + stubs
- **Phase 2:** `Playground` — wire the seven-tool forms, response views,
  markdown rendering of reconstruct/summarize output
- **Phase 3:** `Memories` — session memory list with filters, importance
  inline-edit, soft-delete UI

Phases 2 and 3 land as separate PRs.

## Adding a new screen

1. Drop a component in `src/routes/<name>.tsx`.
2. Register the route in `src/App.tsx` under the existing
   `createRoute` blocks.
3. Add a sidebar entry in `src/components/layout/sidebar.tsx` —
   `{ to: "/<name>", label: "<Label>", icon: <LucideIcon /> }`.

Components in `src/components/ui/` follow the shadcn pattern: copy a
canonical component from <https://ui.shadcn.com/docs/components>,
inline it, adapt to our `cn()` helper. No `npx shadcn add` automation
because owning the components in-tree is the whole point.

## Production deploy (preview)

The current scaffold builds to `web/dist/`. v0.2.1 lands the
`rust-embed` integration that bundles `dist/` into the substrate binary
itself, so `cargo run -- serve` serves both API and dashboard from one
process. Until then, host `dist/` anywhere static (Vercel, Netlify,
Cloudflare Pages) and set `VITE_API_BASE_URL` at build time to point at
your substrate deployment.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| Dashboard loads but every API call 502s | Substrate isn't running on `:8080` | Start it: `cargo run -- serve` from the repo root |
| `tailwindcss` errors on `pnpm dev` | Cached node_modules from a v3 install | `rm -rf node_modules pnpm-lock.yaml && pnpm install` |
| TanStack Router type errors after adding a route | Router type-tree out of date | Restart `pnpm dev` so the codegen picks up the new route |
| Production build is huge | Probably bundling lucide-react un-tree-shaken | Import icons individually: `import { Home } from "lucide-react"` (not `import * as Lucide`) |
