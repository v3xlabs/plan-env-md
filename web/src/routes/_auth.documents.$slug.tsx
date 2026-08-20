import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/solid-router";
import clsx from "clsx";
import { createSignal, For, Show, Suspense } from "solid-js";

import {
  deleteDocument,
  documentQueryOptions,
  publishDocument,
  refreshPreview,
  unpublishDocument,
} from "../api/documents";
import { projectsQueryOptions } from "../api/projects";
import { Button } from "../components/Button";
import { CopyBlock } from "../components/CopyBlock";
import { iconForTag } from "../components/Icon";
import { Modal } from "../components/Modal";
import { ProjectFavicon } from "../components/ProjectFavicon";
import { TextInput } from "../components/TextInput";
import { Thumbnail } from "../components/Thumbnail";

const formatSize = (sizeBytes: number) =>
  (sizeBytes < 1024 ? `${sizeBytes} B` : `${(sizeBytes / 1024).toFixed(1)} KB`);

/// No look-alike characters, and nothing that needs escaping in a URL, since
/// these passwords are read off a screen and pasted into links.
const ALPHABET = "abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const PASSWORD_LENGTH = 20;

const generatePassword = () => {
  // bytes at or above the last whole multiple of the alphabet would make the
  // first few characters likelier than the rest, so they are drawn again
  const ceiling = 256 - (256 % ALPHABET.length);
  const password: string[] = [];

  while (password.length < PASSWORD_LENGTH) {
    const draw = crypto.getRandomValues(new Uint8Array(PASSWORD_LENGTH));

    for (const byte of draw) {
      if (byte < ceiling && password.length < PASSWORD_LENGTH) {
        password.push(ALPHABET.charAt(byte % ALPHABET.length));
      }
    }
  }

  return password.join("");
};

/// The gate reads the password out of the fragment. A fragment never reaches
/// the server, so it stays out of request logs.
const linkWithPassword = (url: string, password: string) =>
  `${url}#k=${encodeURIComponent(password)}`;

const DocumentPage = () => {
  const parameters = Route.useParams();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const detail = useQuery(() => documentQueryOptions(parameters().slug));
  const projects = useQuery(() => projectsQueryOptions);

  /// Whether the project has an icon lives on the project, not the document,
  /// so the list is what answers it.
  const hasProjectIcon = (slug: string) => {
    const project = projects.data?.find(entry => entry.slug === slug);

    return Boolean(project?.has_favicon_light ?? project?.has_favicon_dark);
  };

  const [isShareOpen, setShareOpen] = createSignal(false);
  const [isDeleteOpen, setDeleteOpen] = createSignal(false);
  /// A published document shows its controls first; the password field only
  /// appears once the reader asks to replace the password.
  const [isRotating, setRotating] = createSignal(false);
  const [password, setPassword] = createSignal("");
  /// The password this session just published with, which is what makes a
  /// link that opens without typing anything. It is gone on reload.
  const [shared, setShared] = createSignal<string>();
  const [justCopied, setJustCopied] = createSignal(false);

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ["docs"] });
  };

  const publish = useMutation(() => ({
    mutationFn: publishDocument,
    // the server only ever stores a hash, so this is the one moment the link
    // can be built. Losing it means rotating to get a new one.
    onSuccess: (_, variables) => {
      invalidate();
      setRotating(false);
      setShared(variables.password);
      setPassword("");
    },
  }));

  const unpublish = useMutation(() => ({
    mutationFn: unpublishDocument,
    onSuccess: () => {
      invalidate();
      setShared();
    },
  }));

  const rerender = useMutation(() => ({
    mutationFn: refreshPreview,
    onSuccess: invalidate,
  }));

  /// The worker renders in the background, so the button reports that the job
  /// was accepted rather than pretending the picture has already changed.
  const rerenderLabel = () => {
    if (rerender.isPending) return "queueing";

    if (rerender.isError) return "could not queue";

    return rerender.isSuccess ? "queued, reload shortly" : "re-render preview";
  };

  const remove = useMutation(() => ({
    mutationFn: deleteDocument,
    onSuccess: async () => {
      invalidate();
      await navigate({ to: "/" });
    },
  }));

  /// Generates a password, publishes with it, and puts the ready to send link
  /// on the clipboard, for when none of the three steps are interesting.
  const shareInOneStep = async (slug: string) => {
    const generated = generatePassword();
    const url = await publish.mutateAsync({ slug, password: generated });

    await navigator.clipboard.writeText(linkWithPassword(url, generated));
    setJustCopied(true);
    setTimeout(() => setJustCopied(false), 1500);
  };

  return (
    <Suspense fallback={<p class="text-muted">Loading document.</p>}>
      <Show when={detail.data}>
        {document => (
          <div class="space-y-8">
            <header class="flex flex-wrap items-start gap-5">
              <div class="shrink-0 space-y-1.5">
                <Thumbnail
                  slug={document().slug}
                  href={document().url}
                  class="h-40 w-64"
                />
                {/* beside the thumbnail, because the thumbnail is the thing it
                    is about: a preview captured before the page's own assets
                    existed stays wrong until it is asked for again */}
                <button
                  type="button"
                  disabled={rerender.isPending}
                  onClick={() => rerender.mutate(document().slug)}
                  class="font-mono text-xs text-muted hover:text-accent disabled:opacity-50"
                >
                  {rerenderLabel()}
                </button>
              </div>

              <div class="min-w-0 flex-1 space-y-2">
                <Show when={document().project}>
                  {project => (
                    <Link
                      to="/projects/$project"
                      params={{ project: project() }}
                      class="flex w-fit items-center gap-1.5 font-mono text-xs text-muted hover:text-accent"
                    >
                      <ProjectFavicon
                        project={project()}
                        has={hasProjectIcon(project())}
                        class="size-4"
                      />
                      {project()}
                    </Link>
                  )}
                </Show>

                <h1 class="text-xl font-semibold">{document().title ?? document().slug}</h1>

                <div class="flex flex-wrap items-center gap-x-3 gap-y-1.5 font-mono text-xs text-muted">
                  <span
                    class={clsx(
                      "rounded border px-1.5 py-0.5",
                      document().published
                        ? "border-accent/40 text-accent"
                        : "border-line text-muted",
                    )}
                  >
                    {document().published ? "published" : "private"}
                  </span>
                  <span class="truncate">{document().slug}</span>
                  <time datetime={document().created_at}>{document().created_at}</time>
                  <For each={document().tags}>
                    {(tag) => {
                      const TagIcon = iconForTag(tag);

                      return (
                        <span class="flex items-center gap-1">
                          <TagIcon class="text-sm" />
                          {tag}
                        </span>
                      );
                    }}
                  </For>
                </div>
              </div>

              <div class="flex shrink-0 gap-2">
                <Button variant="quiet" onClick={() => setShareOpen(true)}>
                  Share
                </Button>
                <a
                  href={document().url}
                  class="rounded-md bg-accent px-3 py-1.5 text-sm font-medium text-accent-contrast hover:opacity-90"
                >
                  Open
                </a>
              </div>
            </header>

            <section>
              <h2 class="mb-2 font-mono text-xs tracking-wide text-muted uppercase">
                Revisions
              </h2>
              <ol class="divide-y divide-line rounded-lg border border-line">
                <For each={document().revisions.toReversed()}>
                  {(revision, index) => (
                    <li class="flex items-center gap-4 px-3 py-2 font-mono text-xs">
                      <span class="w-12 font-medium text-ink">
                        {`rev ${revision.revision}`}
                      </span>
                      <span class="w-14 tracking-wide text-accent uppercase">
                        {index() === 0 ? "current" : ""}
                      </span>
                      <time datetime={revision.created_at} class="flex-1 text-muted">
                        {revision.created_at}
                      </time>
                      <span class="w-16 text-right text-muted">
                        {formatSize(revision.size_bytes)}
                      </span>
                      <a
                        href={
                          index() === 0
                            ? document().url
                            : `${document().url}/rev/${revision.revision}`
                        }
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

            <section>
              <h2 class="mb-2 font-mono text-xs tracking-wide text-muted uppercase">
                Delete this document
              </h2>
              <div class="flex items-center gap-3">
                <Button variant="danger" onClick={() => setDeleteOpen(true)}>
                  Delete
                </Button>
                <p class="min-w-0 flex-1 text-xs text-muted">
                  Every revision goes with it, including the links people already
                  hold. This cannot be undone.
                </p>
              </div>
            </section>

            <Modal
              title="Delete this document"
              isOpen={isDeleteOpen()}
              onOpenChange={setDeleteOpen}
            >
              <div class="space-y-4">
                <p class="text-sm text-muted">
                  {document().title ?? document().slug}
                  {" and its "}
                  {document().revisions.length}
                  {document().revisions.length === 1 ? " revision" : " revisions"}
                  {" are removed. Anyone holding a link gets nothing. This cannot be undone."}
                </p>
                <Show when={remove.error}>
                  {error => (
                    <p class="text-sm text-red-700 dark:text-red-400">{error().message}</p>
                  )}
                </Show>
                <div class="flex justify-end gap-2">
                  <Button variant="quiet" onClick={() => setDeleteOpen(false)}>
                    Cancel
                  </Button>
                  <Button
                    variant="danger"
                    disabled={remove.isPending}
                    onClick={() => remove.mutate(document().slug)}
                  >
                    {remove.isPending ? "Deleting" : "Delete"}
                  </Button>
                </div>
              </div>
            </Modal>

            <Modal title="Share" isOpen={isShareOpen()} onOpenChange={setShareOpen}>
              <div class="space-y-4">
                <CopyBlock text={document().url} />

                <p class="text-sm text-muted">
                  {document().published
                    ? "Anyone with the link and the document password can read it. One password opens every revision, including future ones."
                    : "Only you can open it, signed in or with an API token. Publish it with a password to let anyone with the link read it."}
                </p>

                {/* Publishing and rotating are the same request, so they are one
                    form rather than two dialogs the reader has to choose between. */}
                <Show
                  when={!document().published || isRotating()}
                  fallback={(
                    <div class="flex flex-wrap justify-end gap-2">
                      <Button
                        variant="danger"
                        disabled={unpublish.isPending}
                        onClick={() => unpublish.mutate(document().slug)}
                      >
                        Unpublish
                      </Button>
                      <Button variant="quiet" onClick={() => setRotating(true)}>
                        Rotate password
                      </Button>
                    </div>
                  )}
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
                    <div class="flex items-end gap-2">
                      <div class="min-w-0 flex-1">
                        <TextInput
                          label="Document password"
                          type="text"
                          required
                          value={password()}
                          onInput={event => setPassword(event.currentTarget.value)}
                        />
                      </div>
                      <Button
                        variant="quiet"
                        class="shrink-0"
                        onClick={() => setPassword(generatePassword())}
                      >
                        Generate
                      </Button>
                    </div>
                    <Show when={document().published}>
                      <p class="text-xs text-muted">
                        Rotating locks out everyone holding the old password.
                      </p>
                    </Show>
                    <Show when={publish.error}>
                      {error => (
                        <p class="text-sm text-red-700 dark:text-red-400">
                          {error().message}
                        </p>
                      )}
                    </Show>
                    <div class="flex flex-wrap items-center justify-end gap-2">
                      <Button
                        variant="quiet"
                        class="mr-auto"
                        disabled={publish.isPending}
                        onClick={() => void shareInOneStep(document().slug)}
                      >
                        {justCopied() ? "Link copied" : "Generate and copy link"}
                      </Button>
                      <Show when={document().published}>
                        <Button variant="quiet" onClick={() => setRotating(false)}>
                          Cancel
                        </Button>
                      </Show>
                      <Button type="submit" disabled={publish.isPending}>
                        {document().published ? "Rotate" : "Publish"}
                      </Button>
                    </div>
                  </form>
                </Show>

                {/* Only offered for a password this session set, because the
                    server keeps a hash and cannot hand an old one back. */}
                <Show when={document().published && shared()}>
                  {password => (
                    <div class="space-y-2 border-t border-line pt-4">
                      <h3 class="font-mono text-xs tracking-wide text-muted uppercase">
                        Link with password
                      </h3>
                      <CopyBlock text={linkWithPassword(document().url, password())} />
                      <p class="text-xs text-muted">
                        Opens without typing anything. The password rides in the
                        fragment, so it never reaches the server log, but anyone
                        holding the link is inside. It is shown until you leave
                        this page.
                      </p>
                    </div>
                  )}
                </Show>
              </div>
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
