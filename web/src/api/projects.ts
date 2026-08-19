import { queryOptions } from "@tanstack/solid-query";

import { api } from "./fetch";
import type { components } from "./schema.gen";

export type ProjectSummary = components["schemas"]["ProjectBody"];

export const projectsQueryOptions = queryOptions({
  queryKey: ["projects"],
  queryFn: async (): Promise<ProjectSummary[]> => {
    const response = await api("/api/projects", "get", {});

    if (response.status === 200) return response.data;

    throw new Error(`Could not load projects (status ${response.status})`);
  },
});

export const faviconUrl = (project: string, scheme: "light" | "dark") =>
  `/api/projects/${encodeURIComponent(project)}/favicon?scheme=${scheme}`;

export const addAlias = async (input: { project: string; alias: string; }): Promise<void> => {
  const response = await api("/api/projects/{project}/aliases/{alias}", "put", {
    path: { project: input.project, alias: input.alias },
  });

  if (response.status === 204) return;

  if (response.status === 409 || response.status === 422) throw new Error(response.data);

  throw new Error(`Could not add the alias (status ${response.status})`);
};

export const removeProject = async (project: string): Promise<void> => {
  const response = await api("/api/projects/{project}", "delete", { path: { project } });

  if (response.status === 204) return;

  if (response.status === 409) throw new Error(response.data);

  throw new Error(`Could not remove the project (status ${response.status})`);
};

export const removeAlias = async (input: { project: string; alias: string; }): Promise<void> => {
  const response = await api("/api/projects/{project}/aliases/{alias}", "delete", {
    path: { project: input.project, alias: input.alias },
  });

  if (response.status !== 204) {
    throw new Error(`Could not remove the alias (status ${response.status})`);
  }
};

export const setFavicon = async (input: {
  project: string;
  scheme: "light" | "dark";
  file: File;
}): Promise<void> => {
  // the API sniffs the bytes rather than trusting a declared type, so the
  // browser's guess at the file's type is not worth forwarding
  const response = await fetch(faviconUrl(input.project, input.scheme), {
    method: "PUT",
    body: input.file,
  });

  if (response.status === 422) throw new Error(await response.text());

  if (!response.ok) throw new Error(`Upload failed (status ${response.status})`);
};
