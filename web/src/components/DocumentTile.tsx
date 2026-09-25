import { Link } from "@tanstack/solid-router";
import { Show } from "solid-js";

import type { DocumentSummary } from "../api/documents";
import { Answered, PushedAt, TagMark, Visibility } from "./DocumentMarks";
import { ProjectFavicon } from "./ProjectFavicon";
import { Thumbnail } from "./Thumbnail";

type Properties = {
  document: DocumentSummary;
  /// A project page already says which project it is, so the label is noise there
  showProject?: boolean;
};

/// Half the height of a list row. The preview shrinks to a stamp: enough to
/// tell two documents apart, not enough to read.
export const DocumentTile = (properties: Properties) => {
  const document = () => properties.document;

  return (
    <Link
      to="/documents/$slug"
      params={{ slug: document().slug }}
      class="group flex h-full min-w-0 items-center gap-3 px-4 py-2.5 hover:bg-raised"
    >
      <Thumbnail slug={document().slug} class="h-10 w-16 rounded-md" />

      <div class="min-w-0 flex-1">
        <p class="truncate text-sm font-medium text-slate-900 dark:text-slate-100">
          {document().title ?? document().slug}
        </p>
        <p class="flex min-w-0 items-center gap-1.5 text-xs text-slate-500">
          <Show when={(properties.showProject ?? true) && document().project}>
            {project => (
              <>
                <span class="flex min-w-0 items-center gap-1.5 text-slate-600 dark:text-slate-400">
                  <ProjectFavicon project={project()} class="size-4" />
                  <span class="truncate">{project()}</span>
                </span>
                <span aria-hidden="true">-</span>
              </>
            )}
          </Show>
          <span class="shrink-0">{`rev ${document().latest_revision}`}</span>
        </p>
      </div>

      <div class="flex shrink-0 items-center gap-2.5 text-xs text-slate-500">
        <Answered document={document()} />
        <Show when={document().tags[0]}>
          {tag => <TagMark tag={tag()} />}
        </Show>
        <Visibility published={document().published} />
        <PushedAt at={document().last_pushed_at} />
      </div>
    </Link>
  );
};
