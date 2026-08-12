import { queryOptions } from "@tanstack/solid-query";

import { api, JSON_BODY } from "./fetch";
import type { components } from "./schema.gen";

export type User = components["schemas"]["UserBody"];

export const meQueryOptions = queryOptions({
  queryKey: ["me"],
  queryFn: async (): Promise<User | null> => {
    const response = await api("/api/auth/me", "get", {});

    if (response.status === 200) return response.data;

    if (response.status === 401) return null;

    throw new Error(`Could not load the session (status ${response.status})`);
  },
  staleTime: 30_000,
});

type Credentials = {
  username: string;
  password: string;
};

export const login = async (credentials: Credentials): Promise<User> => {
  const response = await api("/api/auth/login", "post", {
    contentType: JSON_BODY,
    data: credentials,
  });

  if (response.status === 200) return response.data;

  if (response.status === 401) throw new Error("Unknown username or wrong password.");

  throw new Error(`Login failed (status ${response.status})`);
};

export const register = async (
  credentials: Credentials & { invite_code?: string; },
): Promise<User> => {
  const response = await api("/api/auth/register", "post", {
    contentType: JSON_BODY,
    data: credentials,
  });

  if (response.status === 200) return response.data;

  if (response.status === 403) throw new Error("Invite code missing, unknown, or already used.");

  if (response.status === 409) throw new Error("Username already taken.");

  if (response.status === 422) throw new Error(response.data);

  throw new Error(`Registration failed (status ${response.status})`);
};

export const logout = async (): Promise<void> => {
  const response = await api("/api/auth/logout", "post", {});

  if (response.status !== 204) throw new Error(`Logout failed (status ${response.status})`);
};
