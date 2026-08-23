import { useQuery } from "@tanstack/solid-query";
import { createFileRoute, Link } from "@tanstack/solid-router";
import { For, Show, Suspense } from "solid-js";

import { documentsQueryOptions, type DocumentSummary } from "../api/documents";
import { CopyBlock } from "../components/CopyBlock";
import { DocumentCollection } from "../components/DocumentCollection";
import { iconForTag } from "../components/Icon";
import { ProjectFavicon } from "../components/ProjectFavicon";
import { parseView, type View, ViewToggle } from "../components/ViewToggle";
import { dayLabel, weekLabel } from "../time";

const pushCommand = (slug: string) =>
  [
    `curl -sS -X PUT "${globalThis.location.origin}/api/docs/${slug}" \\`,
    "  -H \"Authorization: Bearer $(cat ~/.config/plan-env-md/config)\" \\",
    "  -F 'meta={\"title\":\"My plan\",\"project\":\"myproject\",\"tags\":[\"plan\"]};type=application/json' \\",
    "  -F 'index.html=@plan.html;type=text/html'",
  ].join("\n");

const VIEWS = ["list", "cards", "tiles", "projects"] as const;

/// Up to this many documents the page is short enough to read whole, and a
/// second way to look at it is one more control for nothing.
const VIEW_CHOICE_THRESHOLD = 5;

type Search = {
  tag?: string;
  view?: View;
};

type DayGroup = {
  label: string;
  documents: DocumentSummary[];
};

// One bucket size for the whole list, chosen once. Mixed bucket sizes mean the
// reader has to work out what a heading means for every heading.
const groupByDate = (documents: DocumentSummary[]): DayGroup[] => {
  const days = new Set(documents.map(document => dayLabel(document.last_pushed_at)));
  const isSparse = days.size > 10 && documents.length / days.size < 2;
  const labelOf = (document: DocumentSummary) =>
    (isSparse ? weekLabel(document.last_pushed_at) : dayLabel(document.last_pushed_at));

  const groups: DayGroup[] = [];

  for (const document of documents) {
    const label = labelOf(document);
    const group = groups.at(-1);

    if (group?.label === label) {
      group.documents.push(document);
      continue;
    }

    groups.push({ label, documents: [document] });
  }

  return groups;
};

type ProjectGroup = {
  project?: string;
  documents: DocumentSummary[];
};

/// Documents arrive newest first, so a project takes its place in the page
/// from its most recent document and keeps that order inside.
const groupByProject = (documents: DocumentSummary[]): ProjectGroup[] => {
  const byProject = new Map<string, ProjectGroup>();
  const groups: ProjectGroup[] = [];

  for (const document of documents) {
    const key = document.project ?? "";
    const group = byProject.get(key);

    if (group) {
      group.documents.push(document);
      continue;
    }

    const created: ProjectGroup = { project: document.project, documents: [document] };

    byProject.set(key, created);
    groups.push(created);
  }

  return groups;
};

const DocumentsPage = () => {
  const documents = useQuery(() => documentsQueryOptions);
  const search = Route.useSearch();
  const navigate = Route.useNavigate();

  const filtered = () => {
    const all = documents.data ?? [];
    const { tag } = search();

    return tag === undefined ? all : all.filter(document => document.tags.includes(tag));
  };

  const canChooseView = () => filtered().length > VIEW_CHOICE_THRESHOLD;
  /// Below the threshold there is no control to change it back with, so the
  /// page returns to the list rather than stranding the reader in a grid.
  const view = (): View => (canChooseView() ? search().view ?? "list" : "list");

  return (
    <div>
      <div class="mb-6 flex items-center gap-3">
        <h1 class="flex-1 text-xl font-semibold">Documents</h1>
        <Show when={canChooseView()}>
          <ViewToggle
            value={view()}
            views={VIEWS}
            onChange={next => navigate({ search: previous => ({ ...previous, view: next }) })}
          />
        </Show>
      </div>

      <Show when={search().tag}>
        {(tag) => {
          const TagIcon = iconForTag(tag());

          return (
            <p class="mb-4 flex items-center gap-2 text-sm text-muted">
              Filtered to
              <TagIcon class="text-base" />
              <code class="font-mono text-ink">{tag()}</code>
              <Link
                to="/"
                search={previous => ({ view: previous.view })}
                class="text-accent hover:underline"
              >
                clear
              </Link>
            </p>
          );
        }}
      </Show>

      <Suspense fallback={<p class="text-muted">Loading documents.</p>}>
        <Show
          when={filtered().length > 0}
          fallback={(
            <div class="space-y-4 rounded-lg border border-line bg-surface p-6">
              <p>
                No documents yet. Push one with an API token from the
                {" "}
                <Link to="/tokens" class="text-accent hover:underline">
                  Tokens
                </Link>
                {" "}
                page:
              </p>
              <CopyBlock text={pushCommand("myproject-my-plan")} />
            </div>
          )}
        >
          <Show
            when={view() === "projects"}
            fallback={(
              <div class="space-y-8">
                <For each={groupByDate(filtered())}>
                  {group => (
                    <section aria-label={group.label}>
                      <h2 class="mb-2 font-mono text-xs font-medium tracking-wide text-muted uppercase">
                        {group.label}
                      </h2>
                      <DocumentCollection documents={group.documents} view={view()} />
                    </section>
                  )}
                </For>
              </div>
            )}
          >
            <div class="space-y-7">
              <For each={groupByProject(filtered())}>
                {group => (
                  <section aria-label={group.project ?? "No project"}>
                    <header class="mb-3 flex items-center gap-2 border-b border-line pb-2">
                      <Show when={group.project} fallback={<h2 class="font-mono text-sm font-medium">No project</h2>}>
                        {project => (
                          <>
                            <ProjectFavicon project={project()} has class="size-5" />
                            <h2 class="font-mono text-sm font-medium">{project()}</h2>
                          </>
                        )}
                      </Show>
                      <p class="font-mono text-xs text-muted">
                        {group.documents.length}
                        {group.documents.length === 1 ? " document" : " documents"}
                      </p>
                      <Show when={group.project}>
                        {project => (
                          <Link
                            to="/projects/$project"
                            params={{ project: project() }}
                            class="ml-auto font-mono text-xs text-muted hover:text-ink"
                          >
                            open project
                          </Link>
                        )}
                      </Show>
                    </header>
                    <DocumentCollection
                      documents={group.documents}
                      view="cards"
                      showProject={false}
                    />
                  </section>
                )}
              </For>
            </div>
          </Show>
        </Show>
      </Suspense>
    </div>
  );
};

export const Route = createFileRoute("/_auth/")({
  validateSearch: (search: Record<string, unknown>): Search => ({
    tag: typeof search.tag === "string" ? search.tag : undefined,
    view: parseView(search.view),
  }),
  component: DocumentsPage,
});
