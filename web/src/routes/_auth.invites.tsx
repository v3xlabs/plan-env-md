import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute, redirect } from "@tanstack/solid-router";
import { For, Show, Suspense } from "solid-js";

import { deleteInvite, invitesQueryOptions, mintInvite } from "../api/invites";
import { Button } from "../components/Button";
import { EMPTY, LIST, PAGE_TITLE, STATUS } from "../components/Control";

const InvitesPage = () => {
  const queryClient = useQueryClient();
  const invites = useQuery(() => invitesQueryOptions);

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ["invites"] });
  };

  const mint = useMutation(() => ({
    mutationFn: mintInvite,
    onSuccess: invalidate,
  }));

  const remove = useMutation(() => ({
    mutationFn: deleteInvite,
    onSuccess: invalidate,
  }));

  return (
    <div class="space-y-6">
      <div class="flex items-center justify-between gap-4">
        <h1 class={PAGE_TITLE}>Invites</h1>
        <Button disabled={mint.isPending} onClick={() => mint.mutate()}>
          {mint.isPending ? "Minting..." : "Mint invite"}
        </Button>
      </div>

      <Suspense fallback={<p class={STATUS}>Loading invites...</p>}>
        <Show
          when={invites.data && invites.data.length > 0}
          fallback={<p class={EMPTY}>No invites minted yet.</p>}
        >
          <ul class={LIST}>
            <For each={invites.data}>
              {invite => (
                <li class="flex items-center gap-4 px-5 py-3.5 text-sm">
                  <code class="font-mono">{invite.code}</code>
                  <span class="flex-1 text-xs text-slate-500">
                    {invite.used_by ? `used by ${invite.used_by}` : "unused"}
                  </span>
                  <Show when={!invite.used_by}>
                    <Button
                      variant="secondary"
                      disabled={remove.isPending}
                      // eslint-disable-next-line no-restricted-syntax -- `id` is the API field name
                      onClick={() => remove.mutate(invite.id)}
                    >
                      Delete
                    </Button>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Suspense>
    </div>
  );
};

export const Route = createFileRoute("/_auth/invites")({
  beforeLoad: ({ context }) => {
    if (!context.user.is_admin) throw redirect({ to: "/" });
  },
  component: InvitesPage,
});
