import { DropdownMenu } from "@kobalte/core/dropdown-menu";
import type { QueryClient } from "@tanstack/solid-query";
import { useQuery, useQueryClient } from "@tanstack/solid-query";
import { createRootRouteWithContext, Link, Outlet, useNavigate } from "@tanstack/solid-router";
import { TbOutlineChevronDown, TbOutlineKey, TbOutlineLogout, TbOutlineTicket } from "solid-icons/tb";
import { Show } from "solid-js";

import { logout, meQueryOptions } from "../api/auth";
import { MENU_ITEM, MENU_ITEM_DANGER, POPOVER, TRIGGER } from "../components/Control";
import { Logo } from "../components/Logo";
import { ThemeToggle } from "../components/ThemeToggle";

const NAV_ITEM = "rounded-control px-2 py-1 text-sm text-slate-600 hover:bg-raised hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100";

const USER_TRIGGER = `${TRIGGER} max-w-48`;
const USER_MENU = `${POPOVER} w-56 space-y-0.5`;

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
    <div class="min-h-screen">
      <header>
        <div class="mx-auto flex max-w-5xl items-center justify-between gap-4 px-6 py-2">
          {/* The mark is the way home, so Documents needs no link of its own. */}
          <nav aria-label="Main" class="flex min-w-0 items-center gap-4">
            <Link
              to="/"
              aria-label="plan.env.md, documents"
              class="flex shrink-0 items-center gap-2 text-sm font-semibold tracking-tight"
            >
              <Logo class="size-6" />
              plan.env.md
            </Link>
            <Show when={me.data}>
              <Link
                to="/projects"
                class={NAV_ITEM}
                activeProps={{ class: "font-medium text-slate-900 dark:text-slate-100" }}
              >
                Projects
              </Link>
            </Show>
          </nav>

          <div class="flex shrink-0 items-center gap-2">
            <ThemeToggle />
            <Show when={me.data}>
              {user => (
                // Tokens and invites are visited once and then not again for
                // weeks, so they sit behind the name rather than in the nav.
                <DropdownMenu placement="bottom-end" gutter={6}>
                  <DropdownMenu.Trigger class={USER_TRIGGER}>
                    <span class="truncate">{user().username}</span>
                    <DropdownMenu.Icon class="flex shrink-0 text-slate-500 dark:text-slate-400">
                      <TbOutlineChevronDown size={14} aria-hidden="true" />
                    </DropdownMenu.Icon>
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Portal>
                    <DropdownMenu.Content class={USER_MENU}>
                      <DropdownMenu.Group>
                        <DropdownMenu.GroupLabel as="div" class="px-2.5 py-1.5">
                          <p class="truncate text-sm font-medium">{user().username}</p>
                          <p class="text-xs text-slate-500 dark:text-slate-400">
                            {user().is_admin ? "Admin" : "Member"}
                          </p>
                        </DropdownMenu.GroupLabel>
                      </DropdownMenu.Group>
                      <DropdownMenu.Separator class="my-1 border-hairline" />
                      <DropdownMenu.Item as={Link} to="/tokens" class={MENU_ITEM}>
                        <TbOutlineKey size={14} aria-hidden="true" />
                        API tokens
                      </DropdownMenu.Item>
                      <Show when={user().is_admin}>
                        <DropdownMenu.Item as={Link} to="/invites" class={MENU_ITEM}>
                          <TbOutlineTicket size={14} aria-hidden="true" />
                          Invites
                        </DropdownMenu.Item>
                      </Show>
                      <DropdownMenu.Separator class="my-1 border-hairline" />
                      <DropdownMenu.Item onSelect={onLogout} class={MENU_ITEM_DANGER}>
                        <TbOutlineLogout size={14} aria-hidden="true" />
                        Log out
                      </DropdownMenu.Item>
                    </DropdownMenu.Content>
                  </DropdownMenu.Portal>
                </DropdownMenu>
              )}
            </Show>
          </div>
        </div>
      </header>
      <main class="mx-auto max-w-5xl px-6 py-8">
        <Outlet />
      </main>
    </div>
  );
};

export const Route = createRootRouteWithContext<RouterContext>()({
  component: RootLayout,
});
