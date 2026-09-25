import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute, useNavigate } from "@tanstack/solid-router";
import { TbOutlineSettings, TbOutlineX } from "solid-icons/tb";
import { createSignal, For, Show, Suspense } from "solid-js";

import { documentsQueryOptions } from "../api/documents";
import {
  addAlias,
  type ProjectColor,
  projectsQueryOptions,
  removeAlias,
  removeProject,
  setFavicon,
  setProjectColor,
} from "../api/projects";
import { Button } from "../components/Button";
import { BUTTON_SECONDARY, EMPTY, ERROR, FIELD, FOCUS, GLYPH_RAISED, STATUS } from "../components/Control";
import { DocumentCollection } from "../components/DocumentCollection";
import { Modal } from "../components/Modal";
import { derivedColor, PALETTE, PROJECT_COLORS } from "../components/projectColor";
import { ProjectFavicon } from "../components/ProjectFavicon";
import { SettingsSection } from "../components/SettingsSection";
import { preferredView, type View, ViewToggle } from "../components/ViewToggle";

const SCHEMES = ["light", "dark"] as const;

/// Up to this many documents the page is short enough to read whole, and a
/// second way to look at it is one more control for nothing.
const VIEW_CHOICE_THRESHOLD = 5;

const UPLOAD_BUTTON = `${BUTTON_SECONDARY} shrink-0`;
const ALIAS_FIELD = `${FIELD} min-w-0 flex-1 font-mono`;

const SWATCH = `size-6 rounded-full ring-offset-2 ring-offset-surface aria-pressed:ring-2 aria-pressed:ring-slate-900 dark:aria-pressed:ring-slate-100 ${FOCUS}`;
const SWATCHES = Object.fromEntries(
  PALETTE.map(color => [color, `${SWATCH} ${PROJECT_COLORS[color]}`]),
);
const AUTOMATIC = `h-6 rounded-full bg-raised px-2.5 text-xs font-medium text-slate-700 ring-offset-2 ring-offset-surface hover:bg-raised-hover aria-pressed:ring-2 aria-pressed:ring-slate-900 dark:text-slate-300 dark:aria-pressed:ring-slate-100 ${FOCUS}`;

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
    <div class="flex items-center gap-3">
      <ProjectFavicon project={properties.project} scheme={properties.scheme} class="size-8" />
      <div class="min-w-0 flex-1">
        <p class="text-sm font-medium capitalize">{properties.scheme}</p>
        <Show when={error()} fallback={<p class="text-xs text-slate-500">{properties.has ? "Uploaded" : "None yet"}</p>}>
          {message => <p class="text-xs text-red-600 dark:text-red-400" role="alert">{message()}</p>}
        </Show>
      </div>
      <label class={UPLOAD_BUTTON}>
        {upload.isPending ? "Uploading..." : (properties.has ? "Replace" : "Upload")}
        <input
          type="file"
          accept="image/png,image/svg+xml,image/webp,image/gif,image/x-icon,.ico"
          class="sr-only"
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
    <div class="space-y-2">
      <ul class="flex flex-wrap gap-1.5">
        <For each={properties.aliases} fallback={<li class="text-sm text-slate-500">No aliases.</li>}>
          {alias => (
            <li class="flex items-center gap-1 rounded-control bg-raised py-1 pr-1 pl-2.5 font-mono text-xs">
              {alias}
              <button
                type="button"
                onClick={() => remove.mutate(alias)}
                title={`Remove ${alias}`}
                aria-label={`Remove ${alias}`}
                class="flex size-5 items-center justify-center rounded text-slate-500 hover:bg-raised-hover hover:text-slate-900 dark:hover:text-slate-100"
              >
                <TbOutlineX size={12} aria-hidden="true" />
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
          aria-label="New alias"
          class={ALIAS_FIELD}
        />
        <Button
          type="submit"
          variant="secondary"
          class="shrink-0"
          disabled={add.isPending}
        >
          Add
        </Button>
      </form>
      <Show when={error()}>
        {message => <p class={ERROR} role="alert">{message()}</p>}
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
    <div class="space-y-2">
      <div class="flex items-center gap-3">
        <Button
          variant="danger"
          class="shrink-0"
          disabled={!isEmpty() || remove.isPending}
          onClick={() => remove.mutate()}
        >
          {remove.isPending ? "Removing..." : "Remove"}
        </Button>
        <p class="min-w-0 flex-1 text-xs text-slate-500">
          <Show
            when={isEmpty()}
            fallback="Move its documents to another project first."
          >
            Its aliases and icons go too. Documents are not touched.
          </Show>
        </p>
      </div>
      <Show when={error()}>
        {message => <p class={ERROR} role="alert">{message()}</p>}
      </Show>
    </div>
  );
};

/// Automatic keeps the colour derived from the slug, which is also what an
/// unpicked project shows everywhere else.
const ColorPicker = (properties: { project: string; picked: ProjectColor | undefined; }) => {
  const queryClient = useQueryClient();

  const pick = useMutation(() => ({
    mutationFn: (color: ProjectColor | undefined) =>
      setProjectColor({ project: properties.project, color }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["projects"] }),
  }));

  return (
    <div class="space-y-2">
      <div role="group" aria-label="Colour" class="flex flex-wrap items-center gap-2.5">
        <button
          type="button"
          aria-pressed={properties.picked === undefined}
          onClick={() => pick.mutate(undefined)}
          class={AUTOMATIC}
          title={`Automatic, ${derivedColor(properties.project)} for this name`}
        >
          Automatic
        </button>
        <For each={PALETTE}>
          {color => (
            <button
              type="button"
              aria-pressed={properties.picked === color}
              aria-label={color}
              title={color}
              onClick={() => pick.mutate(color)}
              class={SWATCHES[color]}
            />
          )}
        </For>
      </div>
      <Show when={pick.error}>
        {error => <p class={ERROR} role="alert">{error().message}</p>}
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
  const owned = () =>
    (documents.data ?? []).filter(document => document.project === parameters().project);

  const canChooseView = () => owned().length > VIEW_CHOICE_THRESHOLD;
  /// Below the threshold there is no control to change it back with, so the
  /// page shows the list rather than stranding the reader in a grid.
  const view = (): View => (canChooseView() ? preferredView() : "list");

  return (
    <div class="space-y-6">
      <div class="flex items-center gap-3">
        <ProjectFavicon project={parameters().project} class="size-9" />
        <div class="min-w-0 flex-1">
          <h1 class="truncate text-lg font-semibold">{parameters().project}</h1>
          <p class="text-sm text-slate-500 tabular-nums">
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
        <Show when={canChooseView()}>
          <ViewToggle />
        </Show>
        <button
          type="button"
          onClick={() => setSettingsOpen(true)}
          title="Project settings"
          aria-label="Project settings"
          class={GLYPH_RAISED}
        >
          <TbOutlineSettings size={16} aria-hidden="true" />
        </button>
      </div>

      <Modal title="Project settings" isOpen={isSettingsOpen()} onOpenChange={setSettingsOpen}>
        <div class="space-y-5">
          <SettingsSection title="Icon">
            <div class="space-y-2">
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
            </div>
            <p class="text-xs text-slate-500">
              PNG, SVG, WebP, GIF or ICO, up to 64 KB. Square, and legible at 16 pixels.
            </p>
          </SettingsSection>

          <SettingsSection title="Colour">
            <ColorPicker project={parameters().project} picked={project()?.color} />
            <p class="text-xs text-slate-500">
              Shown with the initial wherever the project has no icon.
            </p>
          </SettingsSection>

          <SettingsSection title="Also known as">
            <Aliases project={parameters().project} aliases={project()?.aliases ?? []} />
          </SettingsSection>

          <SettingsSection title="Remove this project">
            <RemoveProject project={parameters().project} documents={owned().length} />
          </SettingsSection>
        </div>
      </Modal>

      <Suspense fallback={<p class={STATUS}>Loading documents...</p>}>
        <Show
          when={owned().length > 0}
          fallback={<p class={EMPTY}>Nothing in this project yet.</p>}
        >
          <DocumentCollection documents={owned()} view={view()} showProject={false} />
        </Show>
      </Suspense>
    </div>
  );
};

export const Route = createFileRoute("/_auth/projects/$project")({
  component: ProjectPage,
});
