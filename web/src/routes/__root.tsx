import type { QueryClient } from "@tanstack/solid-query";
import { useQuery, useQueryClient } from "@tanstack/solid-query";
import { createRootRouteWithContext, Link, Outlet, useNavigate } from "@tanstack/solid-router";
import { Show } from "solid-js";

import { logout, meQueryOptions } from "../api/auth";
import { Button } from "../components/Button";

type RouterContext = {
  queryClient: QueryClient;
};

const RootLayout = () => {
  const me = useQuery(() => meQueryOptions);
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const onLogout = () => {
    void logout().then(() => {
      void queryClient.invalidateQueries({ queryKey: ["me"] });
      void navigate({ to: "/login" });
    });
  };

  return (
    <div class="mx-auto min-h-screen max-w-4xl px-4 py-6">
      <header class="mb-8 flex items-center justify-between gap-4">
        <Link to="/" class="font-mono text-lg font-semibold">
          plan
          <span class="text-accent">.env.md</span>
        </Link>
        <Show when={me.data}>
          {user => (
            <nav class="flex items-center gap-4 text-sm">
              <Link
                to="/"
                class="text-muted hover:text-ink"
                activeProps={{ class: "text-ink font-medium" }}
                activeOptions={{ exact: true }}
              >
                Documents
              </Link>
              <Link
                to="/projects"
                class="text-muted hover:text-ink"
                activeProps={{ class: "text-ink font-medium" }}
              >
                Projects
              </Link>
              <Link
                to="/tokens"
                class="text-muted hover:text-ink"
                activeProps={{ class: "text-ink font-medium" }}
              >
                Tokens
              </Link>
              <Show when={user().is_admin}>
                <Link
                  to="/invites"
                  class="text-muted hover:text-ink"
                  activeProps={{ class: "text-ink font-medium" }}
                >
                  Invites
                </Link>
              </Show>
              <span class="font-mono text-muted">{user().username}</span>
              <Button variant="quiet" onClick={onLogout}>
                Log out
              </Button>
            </nav>
          )}
        </Show>
      </header>
      <main>
        <Outlet />
      </main>
    </div>
  );
};

export const Route = createRootRouteWithContext<RouterContext>()({
  component: RootLayout,
});
