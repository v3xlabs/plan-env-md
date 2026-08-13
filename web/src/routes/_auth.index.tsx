import { useQuery } from "@tanstack/solid-query";
import { createFileRoute, Link } from "@tanstack/solid-router";
import {
  format,
  formatDistanceToNow,
  isToday,
  isYesterday,
  parseISO,
} from "date-fns";
import { For, Show, Suspense } from "solid-js";

import { documentsQueryOptions, type DocumentSummary } from "../api/documents";
import { CopyBlock } from "../components/CopyBlock";

const pushCommand = (slug: string) =>
  [
    `curl -sS -X PUT "${globalThis.location.origin}/api/docs/${slug}" \\`,
    "  -H \"Authorization: Bearer $(cat ~/.config/plan-env-md/config)\" \\",
    "  -H \"Content-Type: text/html\" \\",
    "  --data-binary @plan.html",
  ].join("\n");

const parseTimestamp = (timestamp: string) => parseISO(`${timestamp.replace(" ", "T")}Z`);

const dayLabel = (timestamp: string) => {
  const date = parseTimestamp(timestamp);

  if (isToday(date)) {
    return "Today";
  }

  if (isYesterday(date)) return "Yesterday";

  return format(date, "EEEE, MMMM d");
};

const pushedLabel = (timestamp: string) => {
  const date = parseTimestamp(timestamp);

  return isToday(date)
    ? formatDistanceToNow(date, { addSuffix: true })
    : `at ${format(date, "p")}`;
};

type DocumentDay = {
  label: string;
  documents: DocumentSummary[];
};

const groupDocumentsByDay = (documents: DocumentSummary[]): DocumentDay[] => {
  const days: DocumentDay[] = [];

  for (const document of documents) {
    const label = dayLabel(document.last_pushed_at);
    const day = days.at(-1);

    if (day?.label === label) {
      day.documents.push(document);
      continue;
    }

    days.push({ label, documents: [document] });
  }

  return days;
};

const DocumentsPage = () => {
  const documents = useQuery(() => documentsQueryOptions);

  return (
    <div>
      <h1 class="mb-6 text-xl font-semibold">Documents</h1>
      <Suspense fallback={<p class="text-muted">Loading documents.</p>}>
        <Show
          when={documents.data && documents.data.length > 0}
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
              <CopyBlock text={pushCommand("my-plan")} />
            </div>
          )}
        >
          <div class="space-y-8">
            <For each={groupDocumentsByDay(documents.data ?? [])}>
              {day => (
                <section aria-label={day.label}>
                  <h2 class="mb-2 font-mono text-xs font-medium tracking-wide text-muted uppercase">
                    {day.label}
                  </h2>
                  <ul class="divide-y divide-line border-y border-line">
                    <For each={day.documents}>
                      {document => (
                        <li class="flex items-center gap-4 py-4">
                          <div class="min-w-0 flex-1">
                            <Link
                              to="/documents/$slug"
                              params={{ slug: document.slug }}
                              class="font-mono text-sm font-medium text-ink hover:text-muted"
                            >
                              {document.slug}
                            </Link>
                            <p class="truncate text-sm text-muted">
                              {`${document.title ?? "no title"} - rev ${document.latest_revision} - pushed ${pushedLabel(document.last_pushed_at)}`}
                            </p>
                          </div>
                          <span class="font-mono text-xs tracking-wide text-muted uppercase">
                            {document.published ? "published" : "private"}
                          </span>
                          <a
                            href={document.url}
                            class="text-sm text-muted hover:text-ink hover:underline"
                          >
                            Open
                          </a>
                        </li>
                      )}
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
  component: DocumentsPage,
});
