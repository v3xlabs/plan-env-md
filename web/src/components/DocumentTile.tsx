import { Link } from "@tanstack/solid-router";
import { For, Show } from "solid-js";

import type { DocumentSummary } from "../api/documents";
import { relative } from "../time";
import { iconForTag, LockIcon, PublishedIcon } from "./Icon";
import { ProjectFavicon } from "./ProjectFavicon";
import { Thumbnail } from "./Thumbnail";

type Properties = {
  document: DocumentSummary;
  /// A project page already says which project it is, so the label is noise there
  showProject?: boolean;
};

/// Half the height of a list row, and it keeps the revision and the answer
/// count. The preview shrinks to a stamp: enough to tell two documents apart,
/// not enough to read.
export const DocumentTile = (properties: Properties) => {
  const document = () => properties.document;
  const unanswered = () => document().questions_total - document().questions_answered;

  return (
    <Link
      to="/documents/$slug"
      params={{ slug: document().slug }}
      class="flex min-w-0 items-center gap-2.5 px-3 py-2 hover:bg-surface"
    >
      <Thumbnail slug={document().slug} class="h-10 w-16 rounded border border-line" />

      <div class="min-w-0 flex-1">
        <p class="truncate text-sm font-medium text-ink">
          {document().title ?? document().slug}
        </p>
        <p class="flex min-w-0 items-center gap-1.5 font-mono text-xs text-muted">
          <Show when={(properties.showProject ?? true) && document().project}>
            {project => (
              <span class="flex shrink-0 items-center gap-1 text-ink">
                <ProjectFavicon project={project()} has class="size-3.5" />
                {project()}
              </span>
            )}
          </Show>
          <span class="truncate">
            rev
            {" "}
            {document().latest_revision}
            {" - "}
            <Show
              when={unanswered() > 0}
              fallback={<time datetime={document().last_pushed_at}>{relative(document().last_pushed_at)}</time>}
            >
              <span class="text-accent">
                {unanswered()}
                {" open"}
              </span>
            </Show>
          </span>
        </p>
      </div>

      <div class="flex shrink-0 items-center gap-1.5 text-base text-muted">
        <For each={document().tags.slice(0, 1)}>
          {(tag) => {
            const TagIcon = iconForTag(tag);

            return <TagIcon aria-label={tag} title={tag} />;
          }}
        </For>
        <Show when={document().published} fallback={<LockIcon aria-label="Private" />}>
          <PublishedIcon aria-label="Published" />
        </Show>
      </div>
    </Link>
  );
};
