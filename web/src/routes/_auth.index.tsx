import { DropdownMenu } from "@kobalte/core/dropdown-menu";
import { useQuery } from "@tanstack/solid-query";
import { createFileRoute, Link } from "@tanstack/solid-router";
import { TbOutlineCheck, TbOutlineChevronDown, TbOutlineFolder, TbOutlineSearch } from "solid-icons/tb";
import { For, Show, Suspense } from "solid-js";

import { documentsQueryOptions, type DocumentSummary } from "../api/documents";
import { EMPTY, FOCUS, LINK, MENU_ITEM, PAGE_TITLE, POPOVER, SECTION_TITLE, STATUS, TRIGGER } from "../components/Control";
import { CopyBlock } from "../components/CopyBlock";
import { DocumentCollection } from "../components/DocumentCollection";
import { TagMark } from "../components/DocumentMarks";
import { ProjectFavicon } from "../components/ProjectFavicon";
import { Segmented } from "../components/Segmented";
import { preferredView, type View, ViewToggle } from "../components/ViewToggle";
import { dayLabel, weekLabel } from "../time";

const pushCommand = (slug: string) =>
  [
    `curl -sS -X PUT "${globalThis.location.origin}/api/docs/${slug}" \\`,
    "  -H \"Authorization: Bearer $(cat ~/.config/plan-env-md/config)\" \\",
    "  -F 'meta={\"title\":\"My plan\",\"project\":\"myproject\",\"tags\":[\"plan\"]};type=application/json' \\",
    "  -F 'index.html=@plan.html;type=text/html'",
  ].join("\n");

/// Up to this many documents the page is short enough to read whole, and a
/// second way to look at it is one more control for nothing.
const VIEW_CHOICE_THRESHOLD = 5;

/// A field at toolbar height, with room on the left for its search glyph.
const SEARCH_FIELD = `h-8 w-full rounded-control bg-raised pr-3 pl-8 text-sm text-slate-900 placeholder:text-slate-500 dark:text-slate-100 ${FOCUS}`;

const VISIBILITY_OPTIONS = [
  { value: "all", label: "All" },
  { value: "published", label: "Published" },
  { value: "private", label: "Private" },
] as const;

type Visibility = typeof VISIBILITY_OPTIONS[number]["value"];

const PROJECT_MENU = `${POPOVER} max-h-80 w-60 space-y-0.5 overflow-y-auto`;

type Search = {
  query?: string;
  project?: string;
  visibility?: Exclude<Visibility, "all">;
  tag?: string;
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

const matchesQuery = (document: DocumentSummary, query: string) => {
  const needle = query.toLowerCase();

  return [document.title, document.slug, document.project].some(
    field => field?.toLowerCase().includes(needle),
  );
};

const DocumentsPage = () => {
  const documents = useQuery(() => documentsQueryOptions);
  const search = Route.useSearch();
  const navigate = Route.useNavigate();

  const setSearch = (next: Partial<Search>) =>
    navigate({ search: previous => ({ ...previous, ...next }), replace: true });

  const all = () => documents.data ?? [];

  /// Most used first, so the projects a reader works in sit at the top.
  const projects = () => {
    const counts = new Map<string, number>();

    for (const document of all()) {
      if (document.project) counts.set(document.project, (counts.get(document.project) ?? 0) + 1);
    }

    return [...counts].toSorted((a, b) => b[1] - a[1]);
  };

  const filtered = () => {
    const { query, project, visibility, tag } = search();

    return all().filter(document =>
      (query === undefined || matchesQuery(document, query))
      && (project === undefined || document.project === project)
      && visibility !== (document.published ? "private" : "published")
      && (tag === undefined || document.tags.includes(tag)));
  };

  const canChooseView = () => all().length > VIEW_CHOICE_THRESHOLD;
  /// Below the threshold there is no control to change it back with, so the
  /// page returns to the list rather than stranding the reader in a grid.
  const view = (): View => (canChooseView() ? preferredView() : "list");

  return (
    <div class="space-y-6">
      <h1 class={PAGE_TITLE}>Documents</h1>

      <Show when={all().length > 0}>
        <div class="flex flex-wrap items-center gap-2">
          <label class="relative flex min-w-48 flex-1 items-center">
            <TbOutlineSearch
              size={14}
              aria-hidden="true"
              class="pointer-events-none absolute left-2.5 text-slate-500"
            />
            <input
              type="search"
              aria-label="Filter documents"
              placeholder="Filter by title, slug or project"
              value={search().query ?? ""}
              onInput={event => setSearch({ query: event.currentTarget.value || undefined })}
              class={SEARCH_FIELD}
            />
          </label>

          <DropdownMenu placement="bottom-start" gutter={4}>
            <DropdownMenu.Trigger class={TRIGGER}>
              <TbOutlineFolder size={14} aria-hidden="true" class="text-slate-500" />
              <span class="max-w-40 truncate">{search().project ?? "All projects"}</span>
              <DropdownMenu.Icon class="flex text-slate-500 dark:text-slate-400">
                <TbOutlineChevronDown size={14} aria-hidden="true" />
              </DropdownMenu.Icon>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content class={PROJECT_MENU}>
                <DropdownMenu.RadioGroup
                  value={search().project ?? ""}
                  onChange={value => setSearch({ project: value || undefined })}
                >
                  <DropdownMenu.RadioItem value="" closeOnSelect class={MENU_ITEM}>
                    <span class="flex-1 truncate">All projects</span>
                    <span class="text-xs text-slate-500 tabular-nums">{all().length}</span>
                    <span class="flex w-3.5 justify-center">
                      <DropdownMenu.ItemIndicator>
                        <TbOutlineCheck size={14} aria-hidden="true" />
                      </DropdownMenu.ItemIndicator>
                    </span>
                  </DropdownMenu.RadioItem>
                  <DropdownMenu.Separator class="my-1 border-hairline" />
                  <For each={projects()}>
                    {([project, count]) => (
                      <DropdownMenu.RadioItem value={project} closeOnSelect class={MENU_ITEM}>
                        <ProjectFavicon project={project} class="size-4" />
                        <span class="flex-1 truncate">{project}</span>
                        <span class="text-xs text-slate-500 tabular-nums">{count}</span>
                        <span class="flex w-3.5 justify-center">
                          <DropdownMenu.ItemIndicator>
                            <TbOutlineCheck size={14} aria-hidden="true" />
                          </DropdownMenu.ItemIndicator>
                        </span>
                      </DropdownMenu.RadioItem>
                    )}
                  </For>
                </DropdownMenu.RadioGroup>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu>

          <Segmented
            label="Visibility"
            options={VISIBILITY_OPTIONS}
            value={search().visibility ?? "all"}
            onChange={value => setSearch({ visibility: value === "all" ? undefined : value })}
          />

          <Show when={canChooseView()}>
            <ViewToggle />
          </Show>
        </div>
      </Show>

      <Show when={search().tag}>
        {tag => (
          <p class="flex items-center gap-2 text-sm text-slate-500">
            Tagged
            <span class="text-slate-900 dark:text-slate-100">
              <TagMark tag={tag()} showLabel />
            </span>
            <button type="button" class={LINK} onClick={() => setSearch({ tag: undefined })}>
              Clear
            </button>
          </p>
        )}
      </Show>

      <Suspense fallback={<p class={STATUS}>Loading documents...</p>}>
        <Show
          when={all().length > 0}
          fallback={(
            <div class="space-y-4 rounded-panel bg-surface p-5 text-sm">
              <p class="text-slate-700 dark:text-slate-300">
                No documents yet. Push one with an API token from the
                {" "}
                <Link to="/tokens" class={LINK}>
                  API tokens
                </Link>
                {" "}
                page:
              </p>
              <CopyBlock text={pushCommand("myproject-my-plan")} />
            </div>
          )}
        >
          <Show when={filtered().length > 0} fallback={<p class={EMPTY}>No documents match.</p>}>
            <div class="space-y-6">
              <For each={groupByDate(filtered())}>
                {group => (
                  <section aria-label={group.label} class="space-y-2">
                    <h2 class={SECTION_TITLE}>
                      {group.label}
                      <span class="ml-1.5 font-normal text-slate-500 tabular-nums">
                        {group.documents.length}
                      </span>
                    </h2>
                    <DocumentCollection documents={group.documents} view={view()} />
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

const optionalString = (value: unknown) =>
  (typeof value === "string" && value !== "" ? value : undefined);

export const Route = createFileRoute("/_auth/")({
  validateSearch: (search: Record<string, unknown>): Search => ({
    query: optionalString(search.query),
    project: optionalString(search.project),
    visibility: search.visibility === "published" || search.visibility === "private"
      ? search.visibility
      : undefined,
    tag: optionalString(search.tag),
  }),
  component: DocumentsPage,
});
