import "./style.css";

import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { createRouter, RouterProvider } from "@tanstack/solid-router";
import { render } from "solid-js/web";

import { routeTree } from "./routeTree.gen";

const queryClient = new QueryClient();
const router = createRouter({ routeTree, context: { queryClient } });

declare module "@tanstack/solid-router" {

  interface Register {
    router: typeof router;
  }
}

const root = document.querySelector("#root");

if (!(root instanceof HTMLElement)) throw new Error("missing #root element");

render(
  () => (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  ),
  root,
);
