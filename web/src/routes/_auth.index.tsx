import { useQuery } from "@tanstack/solid-query";
import { createFileRoute, Link } from "@tanstack/solid-router";
import { For, Show, Suspense } from "solid-js";

import { documentsQueryOptions } from "../api/documents";
import { CopyBlock } from "../components/CopyBlock";

const pushCommand = (slug: string) =>
  [
    `curl -sS -X PUT "${globalThis.location.origin}/api/docs/${slug}" \\`,
    "  -H \"Authorization: Bearer $(cat ~/.config/plan-env-md/config)\" \\",
    "  -H \"Content-Type: text/html\" \\",
    "  --data-binary @plan.html",
  ].join("\n");

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
          <ul class="divide-y divide-line rounded-lg border border-line bg-surface">
            <For each={documents.data}>
              {document => (
                <li class="flex items-center gap-4 p-4">
                  <div class="min-w-0 flex-1">
                    <Link
                      to="/documents/$slug"
                      params={{ slug: document.slug }}
                      class="font-mono text-sm font-medium text-accent hover:underline"
                    >
                      {document.slug}
                    </Link>
                    <p class="truncate text-sm text-muted">
                      {`${document.title ?? "no title"} - rev ${document.latest_revision} - updated ${document.updated_at}`}
                    </p>
                  </div>
                  <span
                    class="font-mono text-xs tracking-wide uppercase"
                    classList={{
                      "text-accent": document.published,
                      "text-muted": !document.published,
                    }}
                  >
                    {document.published ? "published" : "private"}
                  </span>
                  <a
                    href={document.url}
                    target="_blank"
                    rel="noreferrer"
                    class="text-sm text-muted hover:text-ink"
                  >
                    Open
                  </a>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Suspense>
    </div>
  );
};

export const Route = createFileRoute("/_auth/")({
  component: DocumentsPage,
});
