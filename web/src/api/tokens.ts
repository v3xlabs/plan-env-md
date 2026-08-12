import { queryOptions } from "@tanstack/solid-query";

import { api, JSON_BODY } from "./fetch";
import type { components } from "./schema.gen";

export type ApiToken = components["schemas"]["TokenBody"];
export type CreatedToken = components["schemas"]["CreatedTokenBody"];

export const tokensQueryOptions = queryOptions({
  queryKey: ["tokens"],
  queryFn: async (): Promise<ApiToken[]> => {
    const response = await api("/api/tokens", "get", {});

    if (response.status === 200) return response.data;

    throw new Error(`Could not load tokens (status ${response.status})`);
  },
});

export const createToken = async (name: string): Promise<CreatedToken> => {
  const response = await api("/api/tokens", "post", {
    contentType: JSON_BODY,
    data: { name },
  });

  if (response.status === 200) return response.data;

  if (response.status === 422) throw new Error(response.data);

  throw new Error(`Token creation failed (status ${response.status})`);
};

export const revokeToken = async (tokenId: number): Promise<void> => {
  const response = await api("/api/tokens/{id}", "delete", {
    // eslint-disable-next-line no-restricted-syntax -- `id` is the API path parameter name
    path: { id: tokenId },
  });

  if (response.status !== 204) throw new Error(`Revoke failed (status ${response.status})`);
};
