import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute, redirect } from "@tanstack/solid-router";
import { For, Show, Suspense } from "solid-js";

import { deleteInvite, invitesQueryOptions, mintInvite } from "../api/invites";
import { Button } from "../components/Button";

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
    <div class="space-y-8">
      <div class="flex items-center justify-between">
        <h1 class="text-xl font-semibold">Invites</h1>
        <Button disabled={mint.isPending} onClick={() => mint.mutate()}>
          Mint invite
        </Button>
      </div>

      <Suspense fallback={<p class="text-muted">Loading invites.</p>}>
        <Show
          when={invites.data && invites.data.length > 0}
          fallback={<p class="text-muted">No invites minted yet.</p>}
        >
          <ul class="divide-y divide-line rounded-lg border border-line bg-surface">
            <For each={invites.data}>
              {invite => (
                <li class="flex items-center gap-4 p-4 text-sm">
                  <code class="font-mono">{invite.code}</code>
                  <span class="flex-1 text-muted">
                    {invite.used_by
                      ? `used by ${invite.used_by}`
                      : "unused"}
                  </span>
                  <Show when={!invite.used_by}>
                    <Button
                      variant="danger"
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
