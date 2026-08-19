import { useQuery } from "@tanstack/solid-query";
import { createFileRoute, Link } from "@tanstack/solid-router";
import { For, Show, Suspense } from "solid-js";

import { documentsQueryOptions, type DocumentSummary } from "../api/documents";
import { CopyBlock } from "../components/CopyBlock";
import { DocumentRow } from "../components/DocumentRow";
import { iconForTag } from "../components/Icon";
import { dayLabel, weekLabel } from "../time";

const pushCommand = (slug: string) =>
  [
    `curl -sS -X PUT "${globalThis.location.origin}/api/docs/${slug}" \\`,
    "  -H \"Authorization: Bearer $(cat ~/.config/plan-env-md/config)\" \\",
    "  -F 'meta={\"title\":\"My plan\",\"project\":\"myproject\",\"tags\":[\"plan\"]};type=application/json' \\",
    "  -F 'index.html=@plan.html;type=text/html'",
  ].join("\n");

type Search = {
  tag?: string;
};

type Group = {
  label: string;
  documents: DocumentSummary[];
};

// One bucket size for the whole list, chosen once. Mixed bucket sizes mean the
// reader has to work out what a heading means for every heading.
const groupByDate = (documents: DocumentSummary[]): Group[] => {
  const days = new Set(documents.map(document => dayLabel(document.last_pushed_at)));
  const isSparse = days.size > 10 && documents.length / days.size < 2;
  const labelOf = (document: DocumentSummary) =>
    (isSparse ? weekLabel(document.last_pushed_at) : dayLabel(document.last_pushed_at));

  const groups: Group[] = [];

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

const DocumentsPage = () => {
  const documents = useQuery(() => documentsQueryOptions);
  const search = Route.useSearch();

  const filtered = () => {
    const all = documents.data ?? [];
    const { tag } = search();

    return tag === undefined ? all : all.filter(document => document.tags.includes(tag));
  };

  return (
    <div>
      <h1 class="mb-6 text-xl font-semibold">Documents</h1>

      <Show when={search().tag}>
        {(tag) => {
          const TagIcon = iconForTag(tag());

          return (
            <p class="mb-4 flex items-center gap-2 text-sm text-muted">
              Filtered to
              <TagIcon class="text-base" />
              <code class="font-mono text-ink">{tag()}</code>
              <Link to="/" search={{}} class="text-accent hover:underline">
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
          <div class="space-y-8">
            <For each={groupByDate(filtered())}>
              {group => (
                <section aria-label={group.label}>
                  <h2 class="mb-2 font-mono text-xs font-medium tracking-wide text-muted uppercase">
                    {group.label}
                  </h2>
                  <ul class="divide-y divide-line border-y border-line">
                    <For each={group.documents}>
                      {document => <DocumentRow document={document} />}
                    </For>
                  </ul>
                </section>
              )}
            </For>
          </div>
        </Show>
      </Suspense>
    </div>
  );
};

export const Route = createFileRoute("/_auth/")({
  validateSearch: (search: Record<string, unknown>): Search => ({
    tag: typeof search.tag === "string" ? search.tag : undefined,
  }),
  component: DocumentsPage,
});
