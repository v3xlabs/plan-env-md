import { DropdownMenu } from "@kobalte/core/dropdown-menu";
import type { QueryClient } from "@tanstack/solid-query";
import { useQuery, useQueryClient } from "@tanstack/solid-query";
import { createRootRouteWithContext, Link, Outlet, useNavigate } from "@tanstack/solid-router";
import { TbOutlineChevronDown } from "solid-icons/tb";
import { Show } from "solid-js";

import { logout, meQueryOptions } from "../api/auth";

const ITEM_CLASS = "flex cursor-default items-center rounded px-2 py-1.5 text-sm text-ink outline-none select-none data-highlighted:bg-bg";

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

              {/* Tokens and invites are visited once and then not again for
                  weeks, so they sit behind the name rather than beside the two
                  places the reader actually works in. */}
              <DropdownMenu>
                <DropdownMenu.Trigger class="flex items-center gap-1 font-mono text-muted hover:text-ink">
                  {user().username}
                  <TbOutlineChevronDown class="size-3.5" aria-hidden="true" />
                </DropdownMenu.Trigger>
                <DropdownMenu.Portal>
                  <DropdownMenu.Content class="z-10 min-w-40 rounded-lg border border-line bg-surface p-1 shadow-lg">
                    <DropdownMenu.Item as={Link} to="/tokens" class={ITEM_CLASS}>
                      Tokens
                    </DropdownMenu.Item>
                    <Show when={user().is_admin}>
                      <DropdownMenu.Item as={Link} to="/invites" class={ITEM_CLASS}>
                        Invites
                      </DropdownMenu.Item>
                    </Show>
                    <DropdownMenu.Separator class="my-1 border-line" />
                    <DropdownMenu.Item onSelect={onLogout} class={ITEM_CLASS}>
                      Log out
                    </DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Portal>
              </DropdownMenu>
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
