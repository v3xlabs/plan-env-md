import { Link } from "@tanstack/solid-router";
import { Show } from "solid-js";

import type { DocumentSummary } from "../api/documents";
import { relative } from "../time";
import { iconForTag, LockIcon, PublishedIcon } from "./Icon";
import { ProjectFavicon } from "./ProjectFavicon";
import { Thumbnail } from "./Thumbnail";

type Properties = {
  document: DocumentSummary;
  /// A project page already says which project it is, so the card gives that
  /// line to the slug instead
  showProject?: boolean;
};

/// The preview is the card. One tag rather than two, because a card carries
/// its subject in the picture and the second icon only crowds it.
export const DocumentCard = (properties: Properties) => {
  const document = () => properties.document;
  const unanswered = () => document().questions_total - document().questions_answered;

  return (
    <Link
      to="/documents/$slug"
      params={{ slug: document().slug }}
      class="block h-full overflow-hidden rounded-lg border border-line bg-surface hover:border-accent"
    >
      <div class="relative aspect-16/10 border-b border-line">
        <Thumbnail slug={document().slug} class="size-full" />
        <div class="absolute top-1.5 right-1.5 flex items-center gap-1 rounded border border-line bg-bg px-1 py-0.5 text-sm text-muted">
          <Show when={document().tags[0]}>
            {(tag) => {
              const TagIcon = iconForTag(tag());

              return <TagIcon aria-label={tag()} title={tag()} />;
            }}
          </Show>
          <Show when={document().published} fallback={<LockIcon aria-label="Private" />}>
            <PublishedIcon aria-label="Published" />
          </Show>
        </div>
      </div>

      <div class="p-2.5">
        {/* Two lines are reserved whether or not the title fills them, so the
            meta lines of a row of cards sit on one baseline. */}
        <p class="line-clamp-2 min-h-[2.75em] text-sm leading-snug font-medium text-ink">
          {document().title ?? document().slug}
        </p>
        <p class="mt-1.5 flex items-center gap-1.5 font-mono text-xs text-muted">
          <Show
            when={(properties.showProject ?? true) && document().project}
            fallback={<span class="truncate">{document().slug}</span>}
          >
            {project => (
              <span class="flex min-w-0 items-center gap-1 text-ink">
                <ProjectFavicon project={project()} has class="size-3.5" />
                <span class="truncate">{project()}</span>
              </span>
            )}
          </Show>
          <Show
            when={unanswered() > 0}
            fallback={(
              <time datetime={document().last_pushed_at} class="ml-auto shrink-0">
                {relative(document().last_pushed_at)}
              </time>
            )}
          >
            <span class="ml-auto shrink-0 text-accent">
              {unanswered()}
              {" open"}
            </span>
          </Show>
        </p>
      </div>
    </Link>
  );
};
