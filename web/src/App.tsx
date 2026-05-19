import {
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";

import { Shell } from "@/components/layout/shell";
import { ProvidersRoute } from "@/routes/providers";
import { PlaygroundRoute } from "@/routes/playground";
import { MemoriesRoute } from "@/routes/memories";

// Code-based routing keeps the scaffold simple — there's no router-generator
// step on the build, no implicit file-system magic. Adding a route is two
// edits: drop the component in `src/routes/`, register it here.

const rootRoute = createRootRoute({
  component: Shell,
});

const providersRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: ProvidersRoute,
});

const playgroundRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/playground",
  component: PlaygroundRoute,
});

const memoriesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/memories",
  component: MemoriesRoute,
});

const routeTree = rootRoute.addChildren([
  providersRoute,
  playgroundRoute,
  memoriesRoute,
]);

export const router = createRouter({
  routeTree,
  defaultPreload: "intent",
});
