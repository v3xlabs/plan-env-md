import { queryOptions } from "@tanstack/solid-query";

import { api, JSON_BODY } from "./fetch";
import type { components } from "./schema.gen";

export type DocumentSummary = components["schemas"]["DocumentBody"];
export type DocumentDetail = components["schemas"]["DocumentDetailBody"];

export const documentsQueryOptions = queryOptions({
  queryKey: ["docs"],
  queryFn: async (): Promise<DocumentSummary[]> => {
    const response = await api("/api/docs", "get", {});

    if (response.status === 200) return response.data;

    throw new Error(`Could not load documents (status ${response.status})`);
  },
});

export const documentQueryOptions = (slug: string) =>
  queryOptions({
    queryKey: ["docs", slug],
    queryFn: async (): Promise<DocumentDetail> => {
      const response = await api("/api/docs/{slug}", "get", {
        path: { slug },
      });

      if (response.status === 200) return response.data;

      if (response.status === 404) throw new Error("No document with this slug.");

      throw new Error(`Could not load the document (status ${response.status})`);
    },
  });

export const publishDocument = async (input: {
  slug: string;
  password: string;
}): Promise<string> => {
  const response = await api("/api/docs/{slug}/publish", "post", {
    path: { slug: input.slug },
    contentType: JSON_BODY,
    data: { password: input.password },
  });

  if (response.status === 200) return response.data.url;

  if (response.status === 422) throw new Error(response.data);

  throw new Error(`Publish failed (status ${response.status})`);
};

export const unpublishDocument = async (slug: string): Promise<void> => {
  const response = await api("/api/docs/{slug}/unpublish", "post", {
    path: { slug },
  });

  if (response.status !== 204) throw new Error(`Unpublish failed (status ${response.status})`);
};
