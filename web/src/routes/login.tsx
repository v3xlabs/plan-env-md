import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute, useNavigate } from "@tanstack/solid-router";
import { createSignal, Show } from "solid-js";

import { login } from "../api/auth";
import { Button } from "../components/Button";
import { ERROR, LINK, PAGE_TITLE } from "../components/Control";
import { TextInput } from "../components/TextInput";

const LoginPage = () => {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [username, setUsername] = createSignal("");
  const [password, setPassword] = createSignal("");

  const mutation = useMutation(() => ({
    mutationFn: login,
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
          mutation.mutate({ username: username(), password: password() });
        }}
      >
        <h1 class={PAGE_TITLE}>Log in</h1>
        <TextInput
          label="Username"
          name="username"
          autocomplete="username"
          required
          value={username()}
          onInput={event => setUsername(event.currentTarget.value)}
        />
        <TextInput
          label="Password"
          name="password"
          type="password"
          autocomplete="current-password"
          required
          value={password()}
          onInput={event => setPassword(event.currentTarget.value)}
        />
        <Show when={mutation.error}>
          {error => <p class={ERROR} role="alert">{error().message}</p>}
        </Show>
        <Button type="submit" class="w-full" disabled={mutation.isPending}>
          {mutation.isPending ? "Logging in..." : "Log in"}
        </Button>
      </form>
      <p class="text-center text-sm text-slate-500">
        Have an invite?
        {" "}
        <a href="/register" class={LINK}>
          Create an account
        </a>
      </p>
    </div>
  );
};

export const Route = createFileRoute("/login")({
  component: LoginPage,
});
