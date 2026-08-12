import { queryOptions } from "@tanstack/solid-query";

import { api } from "./fetch";
import type { components } from "./schema.gen";

export type Invite = components["schemas"]["InviteBody"];

export const invitesQueryOptions = queryOptions({
  queryKey: ["invites"],
  queryFn: async (): Promise<Invite[]> => {
    const response = await api("/api/invites", "get", {});

    if (response.status === 200) return response.data;

    if (response.status === 403) throw new Error("Only admins can manage invites.");

    throw new Error(`Could not load invites (status ${response.status})`);
  },
});

export const mintInvite = async (): Promise<Invite> => {
  const response = await api("/api/invites", "post", {});

  if (response.status === 200) return response.data;

  throw new Error(`Minting failed (status ${response.status})`);
};

export const deleteInvite = async (inviteId: number): Promise<void> => {
  const response = await api("/api/invites/{id}", "delete", {
    // eslint-disable-next-line no-restricted-syntax -- `id` is the API path parameter name
    path: { id: inviteId },
  });

  if (response.status === 404) throw new Error("Invite is already used or gone.");

  if (response.status !== 204) throw new Error(`Delete failed (status ${response.status})`);
};
