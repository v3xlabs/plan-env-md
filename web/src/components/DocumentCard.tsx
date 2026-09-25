import { Link } from "@tanstack/solid-router";
import { Show } from "solid-js";

import type { DocumentSummary } from "../api/documents";
import { Answered, PushedAt, TagMark, Visibility } from "./DocumentMarks";
import { ProjectFavicon } from "./ProjectFavicon";
import { Thumbnail } from "./Thumbnail";

type Properties = {
  document: DocumentSummary;
  /// A project page already says which project it is, so the card gives that
  /// line to the slug instead
  showProject?: boolean;
};

/// The preview is the card: each document is its own surface on the canvas.
export const DocumentCard = (properties: Properties) => {
  const document = () => properties.document;

  return (
    <Link
      to="/documents/$slug"
      params={{ slug: document().slug }}
      class="group flex h-full flex-col overflow-hidden rounded-panel bg-surface hover:bg-raised"
    >
      <Thumbnail slug={document().slug} class="aspect-16/10 w-full" />

      <div class="flex flex-1 flex-col gap-1.5 px-3.5 pt-2.5 pb-3">
        {/* Two lines are reserved whether or not the title fills them, so the
            meta lines of a row of cards sit on one baseline. */}
        <p class="line-clamp-2 min-h-[2.75em] text-sm/snug font-medium text-slate-900 dark:text-slate-100">
          {document().title ?? document().slug}
        </p>
        <p class="flex min-w-0 items-center gap-1.5 text-xs text-slate-500">
          <Show
            when={(properties.showProject ?? true) && document().project}
            fallback={<span class="truncate font-mono">{document().slug}</span>}
          >
            {project => (
              <span class="flex min-w-0 items-center gap-1.5 text-slate-600 dark:text-slate-400">
                <ProjectFavicon project={project()} class="size-4" />
                <span class="truncate">{project()}</span>
              </span>
            )}
          </Show>
        </p>
        <div class="mt-auto flex items-center gap-2.5 pt-1 text-xs text-slate-500">
          <PushedAt at={document().last_pushed_at} class="mr-auto" />
          <Answered document={document()} />
          <Show when={document().tags[0]}>
            {tag => <TagMark tag={tag()} />}
          </Show>
          <Visibility published={document().published} />
        </div>
      </div>
    </Link>
  );
};
