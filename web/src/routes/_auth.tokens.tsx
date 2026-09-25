import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute } from "@tanstack/solid-router";
import { createSignal, For, Show, Suspense } from "solid-js";

import type { CreatedToken } from "../api/tokens";
import { createToken, revokeToken, tokensQueryOptions } from "../api/tokens";
import { Button } from "../components/Button";
import { EMPTY, ERROR, LIST, PAGE_TITLE, STATUS } from "../components/Control";
import { CopyBlock } from "../components/CopyBlock";
import { Modal } from "../components/Modal";
import { TextInput } from "../components/TextInput";

const configCommand = (token: string) =>
  `mkdir -p ~/.config/plan-env-md && echo '${token}' > ~/.config/plan-env-md/config`;

const TokensPage = () => {
  const queryClient = useQueryClient();
  const tokens = useQuery(() => tokensQueryOptions);
  const [name, setName] = createSignal("");
  const [createdToken, setCreatedToken] = createSignal<CreatedToken | null>(null);

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ["tokens"] });
  };

  const create = useMutation(() => ({
    mutationFn: createToken,
    onSuccess: (token) => {
      invalidate();
      setName("");
      setCreatedToken(token);
    },
  }));

  const revoke = useMutation(() => ({
    mutationFn: revokeToken,
    onSuccess: invalidate,
  }));

  return (
    <div class="space-y-6">
      <h1 class={PAGE_TITLE}>API tokens</h1>

      <form
        class="flex items-end gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          create.mutate(name());
        }}
      >
        <div class="flex-1">
          <TextInput
            label="Token name"
            placeholder="laptop-claude"
            required
            value={name()}
            onInput={event => setName(event.currentTarget.value)}
          />
        </div>
        <Button type="submit" disabled={create.isPending}>
          {create.isPending ? "Creating..." : "Create token"}
        </Button>
      </form>
      <Show when={create.error}>
        {error => <p class={ERROR} role="alert">{error().message}</p>}
      </Show>

      <Suspense fallback={<p class={STATUS}>Loading tokens...</p>}>
        <Show
          when={tokens.data && tokens.data.length > 0}
          fallback={<p class={EMPTY}>No tokens yet.</p>}
        >
          <ul class={LIST}>
            <For each={tokens.data}>
              {token => (
                <li class="flex items-center gap-4 px-5 py-3.5">
                  <div class="min-w-0 flex-1">
                    <p class="text-sm font-medium text-slate-900 dark:text-slate-100">{token.name}</p>
                    <p class="flex min-w-0 items-center gap-1.5 text-xs text-slate-500">
                      <span class="font-mono">{`${token.token_prefix}...`}</span>
                      <span aria-hidden="true">-</span>
                      <span>{`created ${token.created_at}`}</span>
                      <span aria-hidden="true">-</span>
                      <span class="truncate">
                        {token.last_used_at ? `last used ${token.last_used_at}` : "never used"}
                      </span>
                    </p>
                  </div>
                  <Button
                    variant="secondary"
                    disabled={revoke.isPending}
                    // eslint-disable-next-line no-restricted-syntax -- `id` is the API field name
                    onClick={() => revoke.mutate(token.id)}
                  >
                    Revoke
                  </Button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Suspense>

      <Modal
        title="Token created"
        isOpen={createdToken() !== null}
        onOpenChange={(isOpen) => {
          if (!isOpen) setCreatedToken(null);
        }}
      >
        <Show when={createdToken()}>
          {token => (
            <div class="space-y-4">
              <p class="text-sm text-slate-600 dark:text-slate-400">
                This is the only time the full token is shown. Store it in the
                agent config file:
              </p>
              <CopyBlock text={configCommand(token().token)} />
              <div class="flex justify-end">
                <Button onClick={() => setCreatedToken(null)}>Done</Button>
              </div>
            </div>
          )}
        </Show>
      </Modal>
    </div>
  );
};

export const Route = createFileRoute("/_auth/tokens")({
  component: TokensPage,
});
