import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute, useNavigate } from "@tanstack/solid-router";
import { createSignal, Show } from "solid-js";

import { register } from "../api/auth";
import { Button } from "../components/Button";
import { ERROR, LINK, PAGE_TITLE } from "../components/Control";
import { TextInput } from "../components/TextInput";

const RegisterPage = () => {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [username, setUsername] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [inviteCode, setInviteCode] = createSignal("");

  const mutation = useMutation(() => ({
    mutationFn: register,
    onSuccess: (user) => {
      queryClient.setQueryData(["me"], user);
      void navigate({ to: "/" });
    },
  }));

  return (
    <div class="mx-auto mt-12 max-w-sm space-y-4">
      <form
        class="space-y-4 rounded-panel bg-surface p-6"
        onSubmit={(event) => {
          event.preventDefault();
          const code = inviteCode().trim();

          mutation.mutate({
            username: username(),
            password: password(),
            ...(code !== "" && { invite_code: code }),
          });
        }}
      >
        <h1 class={PAGE_TITLE}>Create an account</h1>
        <TextInput
          label="Username"
          name="username"
          autocomplete="username"
          required
          pattern="[a-z0-9-]{3,32}"
          title="3 to 32 characters: a-z, 0-9, -"
          value={username()}
          onInput={event => setUsername(event.currentTarget.value)}
        />
        <TextInput
          label="Password"
          name="password"
          type="password"
          autocomplete="new-password"
          required
          minlength={8}
          value={password()}
          onInput={event => setPassword(event.currentTarget.value)}
        />
        <TextInput
          label="Invite code"
          name="invite_code"
          value={inviteCode()}
          onInput={event => setInviteCode(event.currentTarget.value)}
        />
        <p class="text-xs text-slate-500">
          The very first account on a fresh instance needs no invite code.
        </p>
        <Show when={mutation.error}>
          {error => <p class={ERROR} role="alert">{error().message}</p>}
        </Show>
        <Button type="submit" class="w-full" disabled={mutation.isPending}>
          {mutation.isPending ? "Creating..." : "Create account"}
        </Button>
      </form>
      <p class="text-center text-sm text-slate-500">
        Already registered?
        {" "}
        <a href="/login" class={LINK}>
          Log in
        </a>
      </p>
    </div>
  );
};

export const Route = createFileRoute("/register")({
  component: RegisterPage,
});
