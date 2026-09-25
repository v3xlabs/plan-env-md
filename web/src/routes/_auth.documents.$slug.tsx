import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/solid-router";
import { TbOutlineSettings } from "solid-icons/tb";
import { createSignal, For, Show, Suspense } from "solid-js";

import {
  deleteDocument,
  documentQueryOptions,
  publishDocument,
  refreshPreview,
  unpublishDocument,
} from "../api/documents";
import { Button } from "../components/Button";
import {
  BUTTON_PRIMARY,
  ERROR,
  GLYPH_RAISED,
  LINK,
  LIST,
  SECTION_TITLE,
  STATUS,
} from "../components/Control";
import { CopyBlock } from "../components/CopyBlock";
import { TagMark, Visibility } from "../components/DocumentMarks";
import { Modal } from "../components/Modal";
import { ProjectFavicon } from "../components/ProjectFavicon";
import { SettingsSection } from "../components/SettingsSection";
import { TextInput } from "../components/TextInput";
import { Thumbnail } from "../components/Thumbnail";
import { absolute } from "../time";

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

  const [isShareOpen, setShareOpen] = createSignal(false);
  const [isSettingsOpen, setSettingsOpen] = createSignal(false);
  /// Deleting takes a second press rather than a second dialog, since the
  /// settings dialog is already the step that separates it from the page.
  const [isConfirmingDelete, setConfirmingDelete] = createSignal(false);
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
    if (rerender.isPending) return "Queueing...";

    if (rerender.isError) return "Could not queue";

    return rerender.isSuccess ? "Queued" : "Re-render";
  };

  const openSettings = (isOpen: boolean) => {
    setSettingsOpen(isOpen);

    if (!isOpen) setConfirmingDelete(false);
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
    <Suspense fallback={<p class={STATUS}>Loading document...</p>}>
      <Show when={detail.data}>
        {document => (
          <div class="space-y-8">
            <header class="flex flex-wrap items-start gap-5">
              <a
                href={document().url}
                class="group block h-40 w-64 shrink-0 overflow-hidden rounded-panel"
                aria-label="Open the document"
              >
                <Thumbnail slug={document().slug} class="size-full" />
              </a>

              <div class="min-w-0 flex-1 space-y-2">
                <Show when={document().project}>
                  {project => (
                    <Link
                      to="/projects/$project"
                      params={{ project: project() }}
                      class="flex w-fit items-center gap-1.5 text-xs text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
                    >
                      <ProjectFavicon project={project()} class="size-4" />
                      {project()}
                    </Link>
                  )}
                </Show>

                <h1 class="text-lg font-semibold">{document().title ?? document().slug}</h1>

                <div class="flex flex-wrap items-center gap-x-3.5 gap-y-1.5 text-xs text-slate-500">
                  <span class="flex items-center gap-1.5">
                    <Visibility published={document().published} />
                    {document().published ? "Published" : "Private"}
                  </span>
                  <span class="truncate font-mono">{document().slug}</span>
                  <time datetime={document().created_at}>{absolute(document().created_at)}</time>
                  <For each={document().tags}>
                    {tag => <TagMark tag={tag} showLabel />}
                  </For>
                </div>
              </div>

              <div class="flex shrink-0 items-center gap-2">
                <Button variant="secondary" onClick={() => setShareOpen(true)}>
                  Share
                </Button>
                <a href={document().url} class={BUTTON_PRIMARY}>
                  Open
                </a>
                <button
                  type="button"
                  onClick={() => setSettingsOpen(true)}
                  title="Document settings"
                  aria-label="Document settings"
                  class={GLYPH_RAISED}
                >
                  <TbOutlineSettings size={16} aria-hidden="true" />
                </button>
              </div>
            </header>

            <section class="space-y-2">
              <h2 class={SECTION_TITLE}>Revisions</h2>
              <ol class={LIST}>
                <For each={document().revisions.toReversed()}>
                  {(revision, index) => (
                    <li class="flex items-center gap-4 px-5 py-3 text-sm">
                      <span class="w-14 font-medium tabular-nums">
                        {`rev ${revision.revision}`}
                      </span>
                      <span class="w-16 text-xs text-emerald-700 dark:text-emerald-400">
                        {index() === 0 ? "Current" : ""}
                      </span>
                      <time datetime={revision.created_at} class="flex-1 text-xs text-slate-500">
                        {absolute(revision.created_at)}
                      </time>
                      <span class="w-16 text-right text-xs text-slate-500 tabular-nums">
                        {formatSize(revision.size_bytes)}
                      </span>
                      <a
                        href={
                          index() === 0
                            ? document().url
                            : `${document().url}/rev/${revision.revision}`
                        }
                        class={LINK}
                      >
                        Open
                      </a>
                    </li>
                  )}
                </For>
              </ol>
              <p class="text-xs text-slate-500">
                Revision links are permanent and share the document password once
                published.
              </p>
            </section>

            <Modal
              title="Document settings"
              isOpen={isSettingsOpen()}
              onOpenChange={openSettings}
            >
              <div class="space-y-5">
                <SettingsSection title="Preview">
                  <div class="flex items-center gap-3">
                    <Button
                      variant="secondary"
                      class="shrink-0"
                      disabled={rerender.isPending}
                      onClick={() => rerender.mutate(document().slug)}
                    >
                      {rerenderLabel()}
                    </Button>
                    <p class="min-w-0 flex-1 text-xs text-slate-500">
                      The thumbnail is captured once, when the revision is pushed. One
                      taken by an older renderer stays wrong until it is asked for
                      again. Reload the page shortly after.
                    </p>
                  </div>
                </SettingsSection>

                <SettingsSection title="Delete this document">
                  <div class="flex items-center gap-3">
                    <Show
                      when={isConfirmingDelete()}
                      fallback={(
                        <>
                          <Button
                            variant="secondary"
                            class="shrink-0"
                            onClick={() => setConfirmingDelete(true)}
                          >
                            Delete
                          </Button>
                          <p class="min-w-0 flex-1 text-xs text-slate-500">
                            Every revision goes with it, including the links people
                            already hold. This cannot be undone.
                          </p>
                        </>
                      )}
                    >
                      <Button
                        variant="danger"
                        class="shrink-0"
                        disabled={remove.isPending}
                        onClick={() => remove.mutate(document().slug)}
                      >
                        {remove.isPending ? "Deleting..." : "Delete for good"}
                      </Button>
                      <Button
                        variant="secondary"
                        class="shrink-0"
                        onClick={() => setConfirmingDelete(false)}
                      >
                        Cancel
                      </Button>
                      <p class="min-w-0 flex-1 text-xs text-slate-500">
                        {document().revisions.length}
                        {document().revisions.length === 1 ? " revision" : " revisions"}
                        {" go. Anyone holding a link gets nothing."}
                      </p>
                    </Show>
                  </div>
                  <Show when={remove.error}>
                    {error => <p class={ERROR} role="alert">{error().message}</p>}
                  </Show>
                </SettingsSection>
              </div>
            </Modal>

            <Modal title="Share" isOpen={isShareOpen()} onOpenChange={setShareOpen}>
              <div class="space-y-4">
                <CopyBlock text={document().url} />

                <p class="text-sm text-slate-600 dark:text-slate-400">
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
                      <Button variant="secondary" onClick={() => setRotating(true)}>
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
                        variant="secondary"
                        class="shrink-0"
                        onClick={() => setPassword(generatePassword())}
                      >
                        Generate
                      </Button>
                    </div>
                    <Show when={document().published}>
                      <p class="text-xs text-slate-500">
                        Rotating locks out everyone holding the old password.
                      </p>
                    </Show>
                    <Show when={publish.error}>
                      {error => <p class={ERROR} role="alert">{error().message}</p>}
                    </Show>
                    <div class="flex flex-wrap items-center justify-end gap-2">
                      <Button
                        variant="secondary"
                        class="mr-auto"
                        disabled={publish.isPending}
                        onClick={() => void shareInOneStep(document().slug)}
                      >
                        {justCopied() ? "Link copied" : "Generate and copy link"}
                      </Button>
                      <Show when={document().published}>
                        <Button variant="secondary" onClick={() => setRotating(false)}>
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
                    <div class="space-y-2 border-t border-hairline pt-4">
                      <h3 class={SECTION_TITLE}>Link with password</h3>
                      <CopyBlock text={linkWithPassword(document().url, password())} />
                      <p class="text-xs text-slate-500">
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
