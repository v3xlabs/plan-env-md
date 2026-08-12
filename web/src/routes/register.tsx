import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute, useNavigate } from "@tanstack/solid-router";
import { createSignal, Show } from "solid-js";

import { register } from "../api/auth";
import { Button } from "../components/Button";
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
    <div class="mx-auto mt-16 max-w-xs">
      <h1 class="mb-6 text-xl font-semibold">Create an account</h1>
      <form
        class="space-y-4"
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
        <p class="text-xs text-muted">
          The very first account on a fresh instance needs no invite code.
        </p>
        <Show when={mutation.error}>
          {error => <p class="text-sm text-red-700 dark:text-red-400">{error().message}</p>}
        </Show>
        <Button type="submit" class="w-full" disabled={mutation.isPending}>
          {mutation.isPending ? "Creating" : "Create account"}
        </Button>
      </form>
      <p class="mt-4 text-sm text-muted">
        Already registered?
        {" "}
        <a href="/login" class="text-accent hover:underline">
          Log in
        </a>
      </p>
    </div>
  );
};

export const Route = createFileRoute("/register")({
  component: RegisterPage,
});
