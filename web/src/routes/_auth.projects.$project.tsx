import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/solid-router";
import { TbOutlineSettings } from "solid-icons/tb";
import { createSignal, For, type JSX, Show, Suspense } from "solid-js";

import { documentsQueryOptions } from "../api/documents";
import {
  addAlias,
  projectsQueryOptions,
  removeAlias,
  removeProject,
  setFavicon,
} from "../api/projects";
import { DocumentRow } from "../components/DocumentRow";
import { Modal } from "../components/Modal";
import { ProjectFavicon } from "../components/ProjectFavicon";

const SCHEMES = ["light", "dark"] as const;

/// One labelled group in the settings dialog. The dialog holds three unrelated
/// things, and without a heading each they read as one long column of controls.
const Section = (properties: { title: string; children: JSX.Element; }) => (
  <section class="space-y-2">
    <h3 class="font-mono text-xs tracking-wide text-muted uppercase">{properties.title}</h3>
    {properties.children}
  </section>
);

const FaviconSlot = (properties: {
  project: string;
  scheme: "light" | "dark";
  has: boolean;
}) => {
  const queryClient = useQueryClient();
  const [error, setError] = createSignal<string>();

  const upload = useMutation(() => ({
    mutationFn: (file: File) =>
      setFavicon({ project: properties.project, scheme: properties.scheme, file }),
    onSuccess: () => {
      setError();
      queryClient.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: (cause: Error) => setError(cause.message),
  }));

  return (
    <div class="flex items-center gap-3 rounded-lg border border-line p-3">
      <ProjectFavicon
        project={properties.project}
        has={properties.has}
        scheme={properties.scheme}
        class="size-8 shrink-0"
      />
      <div class="min-w-0 flex-1">
        <p class="font-mono text-xs tracking-wide text-muted uppercase">{properties.scheme}</p>
        <Show when={error()}>
          {message => <p class="text-xs text-red-600 dark:text-red-400">{message()}</p>}
        </Show>
      </div>
      <label class="shrink-0 cursor-pointer rounded border border-line px-2 py-1 font-mono text-xs text-muted hover:border-accent hover:text-accent">
        {upload.isPending ? "uploading" : (properties.has ? "replace" : "upload")}
        <input
          type="file"
          accept="image/png,image/svg+xml,image/webp,image/gif,image/x-icon,.ico"
          class="hidden"
          onChange={(event) => {
            const file = event.currentTarget.files?.[0];

            if (file) upload.mutate(file);

            event.currentTarget.value = "";
          }}
        />
      </label>
    </div>
  );
};

/// Other names that resolve to this project on push, so `openlv` and
/// `open-lavatory` do not become two piles.
const Aliases = (properties: { project: string; aliases: string[]; }) => {
  const queryClient = useQueryClient();
  const [draft, setDraft] = createSignal("");
  const [error, setError] = createSignal<string>();
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["projects"] });

  const add = useMutation(() => ({
    mutationFn: (alias: string) => addAlias({ project: properties.project, alias }),
    onSuccess: () => {
      setError();
      setDraft("");
      refresh();
    },
    onError: (cause: Error) => setError(cause.message),
  }));

  const remove = useMutation(() => ({
    mutationFn: (alias: string) => removeAlias({ project: properties.project, alias }),
    onSuccess: refresh,
  }));

  return (
    <div>
      <ul class="mb-2 flex flex-wrap gap-1.5">
        <For each={properties.aliases} fallback={<li class="text-xs text-muted">No aliases.</li>}>
          {alias => (
            <li class="flex items-center gap-1 rounded border border-line px-1.5 py-0.5 font-mono text-xs">
              {alias}
              <button
                type="button"
                onClick={() => remove.mutate(alias)}
                title={`Remove ${alias}`}
                class="text-muted hover:text-ink"
              >
                x
              </button>
            </li>
          )}
        </For>
      </ul>
      <form
        class="flex gap-2"
        onSubmit={(event) => {
          event.preventDefault();

          if (draft().trim()) add.mutate(draft().trim());
        }}
      >
        <input
          value={draft()}
          onInput={event => setDraft(event.currentTarget.value)}
          placeholder="another name"
          class="min-w-0 flex-1 rounded border border-line bg-bg px-2 py-1 font-mono text-xs text-ink"
        />
        <button
          type="submit"
          class="shrink-0 rounded border border-line px-2 py-1 font-mono text-xs text-muted hover:border-accent hover:text-accent"
        >
          add
        </button>
      </form>
      <Show when={error()}>
        {message => <p class="mt-1 text-xs text-red-600 dark:text-red-400">{message()}</p>}
      </Show>
    </div>
  );
};

/// Removing a project is for tidying up one left empty by refiling its
/// documents. The API refuses a project that still holds any, so the button
/// says why before it is pressed rather than after.
const RemoveProject = (properties: { project: string; documents: number; }) => {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [error, setError] = createSignal<string>();
  const isEmpty = () => properties.documents === 0;

  const remove = useMutation(() => ({
    mutationFn: () => removeProject(properties.project),
    onSuccess: async () => {
      setError();
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
      await navigate({ to: "/projects" });
    },
    onError: (cause: Error) => setError(cause.message),
  }));

  return (
    <div>
      <div class="flex items-center gap-3">
        <button
          type="button"
          disabled={!isEmpty() || remove.isPending}
          onClick={() => remove.mutate()}
          class="shrink-0 rounded border border-line px-2 py-1 font-mono text-xs text-muted enabled:hover:border-red-600 enabled:hover:text-red-600 disabled:opacity-50 dark:enabled:hover:border-red-400 dark:enabled:hover:text-red-400"
        >
          {remove.isPending ? "removing" : "remove"}
        </button>
        <p class="min-w-0 flex-1 text-xs text-muted">
          <Show
            when={isEmpty()}
            fallback="Move its documents to another project first."
          >
            Its aliases and icons go too. Documents are not touched.
          </Show>
        </p>
      </div>
      <Show when={error()}>
        {message => <p class="mt-1 text-xs text-red-600 dark:text-red-400">{message()}</p>}
      </Show>
    </div>
  );
};

const ProjectPage = () => {
  const parameters = Route.useParams();
  const documents = useQuery(() => documentsQueryOptions);
  const projects = useQuery(() => projectsQueryOptions);
  const [isSettingsOpen, setSettingsOpen] = createSignal(false);

  const project = () => projects.data?.find(entry => entry.slug === parameters().project);
  const hasIcon = () => Boolean(project()?.has_favicon_light ?? project()?.has_favicon_dark);
  const owned = () =>
    (documents.data ?? []).filter(document => document.project === parameters().project);

  return (
    <div>
      <div class="mb-8 flex items-center gap-3">
        <ProjectFavicon project={parameters().project} has={hasIcon()} class="size-9 shrink-0" />
        <div class="min-w-0 flex-1">
          <h1 class="truncate font-mono text-xl font-semibold">{parameters().project}</h1>
          <p class="text-sm text-muted">
            {owned().length}
            {owned().length === 1 ? " document" : " documents"}
            <Show when={project()?.aliases.length}>
              {count => (
                <>
                  {", "}
                  {count()}
                  {count() === 1 ? " alias" : " aliases"}
                </>
              )}
            </Show>
          </p>
        </div>
        <Link to="/projects" class="shrink-0 text-sm text-muted hover:text-ink hover:underline">
          All projects
        </Link>
        <button
          type="button"
          onClick={() => setSettingsOpen(true)}
          title="Project settings"
          class="shrink-0 text-lg text-muted hover:text-ink"
        >
          <TbOutlineSettings aria-label="Project settings" />
        </button>
      </div>

      <Modal title="Project settings" isOpen={isSettingsOpen()} onOpenChange={setSettingsOpen}>
        <div class="space-y-5">
          <Section title="Icon">
            <For each={SCHEMES}>
              {scheme => (
                <FaviconSlot
                  project={parameters().project}
                  scheme={scheme}
                  has={Boolean(
                    scheme === "light"
                      ? project()?.has_favicon_light
                      : project()?.has_favicon_dark,
                  )}
                />
              )}
            </For>
            <p class="text-xs text-muted">
              PNG, SVG, WebP, GIF or ICO, up to 64 KB. Square, and legible at 16 pixels.
            </p>
          </Section>

          <Section title="Also known as">
            <Aliases project={parameters().project} aliases={project()?.aliases ?? []} />
          </Section>

          <Section title="Remove this project">
            <RemoveProject project={parameters().project} documents={owned().length} />
          </Section>
        </div>
      </Modal>

      <Suspense fallback={<p class="text-muted">Loading documents.</p>}>
        <Show
          when={owned().length > 0}
          fallback={<p class="text-muted">Nothing in this project yet.</p>}
        >
          <ul class="divide-y divide-line border-y border-line">
            <For each={owned()}>
              {document => <DocumentRow document={document} showProject={false} />}
            </For>
          </ul>
        </Show>
      </Suspense>
    </div>
  );
};

export const Route = createFileRoute("/_auth/projects/$project")({
  component: ProjectPage,
});
