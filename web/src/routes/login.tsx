import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute, useNavigate } from "@tanstack/solid-router";
import { createSignal, Show } from "solid-js";

import { login } from "../api/auth";
import { Button } from "../components/Button";
import { TextInput } from "../components/TextInput";

const LoginPage = () => {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [username, setUsername] = createSignal("");
  const [password, setPassword] = createSignal("");

  const mutation = useMutation(() => ({
    mutationFn: login,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["me"] });
      void navigate({ to: "/" });
    },
  }));

  return (
    <div class="mx-auto mt-16 max-w-xs">
      <h1 class="mb-6 text-xl font-semibold">Log in</h1>
      <form
        class="space-y-4"
        onSubmit={(event) => {
          event.preventDefault();
          mutation.mutate({ username: username(), password: password() });
        }}
      >
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
          {error => <p class="text-sm text-red-700 dark:text-red-400">{error().message}</p>}
        </Show>
        <Button type="submit" class="w-full" disabled={mutation.isPending}>
          {mutation.isPending ? "Logging in" : "Log in"}
        </Button>
      </form>
      <p class="mt-4 text-sm text-muted">
        Have an invite?
        {" "}
        <a href="/register" class="text-accent hover:underline">
          Create an account
        </a>
      </p>
    </div>
  );
};

export const Route = createFileRoute("/login")({
  component: LoginPage,
});
