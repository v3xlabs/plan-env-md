import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute } from "@tanstack/solid-router";
import { createSignal, For, Show, Suspense } from "solid-js";

import { documentQueryOptions, publishDocument, unpublishDocument } from "../api/documents";
import { Button } from "../components/Button";
import { CopyBlock } from "../components/CopyBlock";
import { Modal } from "../components/Modal";
import { TextInput } from "../components/TextInput";

const formatSize = (sizeBytes: number) =>
  (sizeBytes < 1024 ? `${sizeBytes} B` : `${(sizeBytes / 1024).toFixed(1)} KB`);

const DocumentPage = () => {
  const parameters = Route.useParams();
  const queryClient = useQueryClient();
  const detail = useQuery(() => documentQueryOptions(parameters().slug));

  const [isPublishOpen, setPublishOpen] = createSignal(false);
  const [password, setPassword] = createSignal("");

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ["docs"] });
  };

  const publish = useMutation(() => ({
    mutationFn: publishDocument,
    onSuccess: () => {
      invalidate();
      setPublishOpen(false);
      setPassword("");
    },
  }));

  const unpublish = useMutation(() => ({
    mutationFn: unpublishDocument,
    onSuccess: invalidate,
  }));

  return (
    <Suspense fallback={<p class="text-muted">Loading document.</p>}>
      <Show when={detail.data}>
        {document => (
          <div class="space-y-8">
            <header class="flex flex-wrap items-start justify-between gap-4">
              <div>
                <h1 class="font-mono text-xl font-semibold">{document().slug}</h1>
                <p class="mt-1 text-sm text-muted">
                  {`${document().title ?? "no title"} - created ${document().created_at}`}
                </p>
              </div>
              <a
                href={document().url}
                target="_blank"
                rel="noreferrer"
                class="rounded-md bg-accent px-3 py-1.5 text-sm font-medium text-accent-contrast hover:opacity-90"
              >
                Open document
              </a>
            </header>

            <section class="space-y-3 rounded-lg border border-line bg-surface p-4">
              <div class="flex flex-wrap items-center justify-between gap-4">
                <div>
                  <h2 class="font-medium">
                    {document().published ? "Published" : "Private"}
                  </h2>
                  <p class="text-sm text-muted">
                    {document().published
                      ? "Anyone with the link and the document password can read it."
                      : "Only you can open it, signed in or with an API token."}
                  </p>
                </div>
                <div class="flex gap-2">
                  <Button onClick={() => setPublishOpen(true)}>
                    {document().published ? "Rotate password" : "Publish"}
                  </Button>
                  <Show when={document().published}>
                    <Button
                      variant="danger"
                      disabled={unpublish.isPending}
                      onClick={() => unpublish.mutate(document().slug)}
                    >
                      Unpublish
                    </Button>
                  </Show>
                </div>
              </div>
              <CopyBlock text={document().url} />
              <Show when={document().published}>
                <p class="text-xs text-muted">
                  One password opens every revision, including future ones.
                  Rotating it locks out everyone who has the old one.
                </p>
              </Show>
            </section>

            <section>
              <h2 class="mb-3 font-medium">Revisions</h2>
              <ol class="divide-y divide-line rounded-lg border border-line bg-surface">
                <For each={document().revisions.toReversed()}>
                  {(revision, index) => (
                    <li class="flex items-center gap-4 p-3 text-sm">
                      <span class="w-14 font-mono font-medium">
                        {`rev ${revision.revision}`}
                      </span>
                      <Show
                        when={index() === 0}
                        fallback={<span class="w-16 font-mono text-xs text-muted" />}
                      >
                        <span class="w-16 font-mono text-xs tracking-wide text-accent uppercase">
                          current
                        </span>
                      </Show>
                      <span class="flex-1 text-muted">
                        {`${revision.created_at} - ${formatSize(revision.size_bytes)}`}
                      </span>
                      <a
                        href={
                          index() === 0
                            ? document().url
                            : `${document().url}/rev/${revision.revision}`
                        }
                        target="_blank"
                        rel="noreferrer"
                        class="text-accent hover:underline"
                      >
                        Open
                      </a>
                    </li>
                  )}
                </For>
              </ol>
              <p class="mt-2 text-xs text-muted">
                Revision links are permanent and share the document password once
                published.
              </p>
            </section>

            <Modal
              title={document().published ? "Rotate the password" : "Publish document"}
              isOpen={isPublishOpen()}
              onOpenChange={setPublishOpen}
            >
              <form
                class="space-y-4"
                onSubmit={(event) => {
                  event.preventDefault();
                  publish.mutate({
                    slug: document().slug,
                    password: password(),
                  });
                }}
              >
                <p class="text-sm text-muted">
                  Visitors to the public URL must enter this password. It covers
                  all revisions, including future ones.
                </p>
                <TextInput
                  label="Document password"
                  type="password"
                  required
                  value={password()}
                  onInput={event => setPassword(event.currentTarget.value)}
                />
                <Show when={publish.error}>
                  {error => (
                    <p class="text-sm text-red-700 dark:text-red-400">
                      {error().message}
                    </p>
                  )}
                </Show>
                <div class="flex justify-end gap-2">
                  <Button variant="quiet" onClick={() => setPublishOpen(false)}>
                    Cancel
                  </Button>
                  <Button type="submit" disabled={publish.isPending}>
                    {document().published ? "Rotate" : "Publish"}
                  </Button>
                </div>
              </form>
            </Modal>
          </div>
        )}
      </Show>
    </Suspense>
  );
};

export const Route = createFileRoute("/_auth/documents/$slug")({
  component: DocumentPage,
});
