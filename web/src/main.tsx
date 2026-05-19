import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";

import { queryClient } from "@/lib/query-client";
import { router } from "@/App";
import "@/index.css";

// Register the router instance for type-safe navigation. Without this
// declaration the type inference on `useNavigate`, `useParams`, etc. is
// stuck at the generic `unknown` type. See:
// https://tanstack.com/router/latest/docs/framework/react/guide/type-safety
declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

const rootEl = document.getElementById("root");
if (!rootEl) {
  throw new Error("Root element #root is missing from index.html");
}

createRoot(rootEl).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
);
