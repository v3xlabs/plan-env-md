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

export const DocumentRow = (properties: Properties) => {
  const document = () => properties.document;

  return (
    <li class="group flex items-center gap-4 px-5 py-3 hover:bg-raised">
      <a href={document().url} class="hidden shrink-0 sm:block" aria-label={`Open ${document().title ?? document().slug}`}>
        <Thumbnail slug={document().slug} class="h-15 w-24 rounded-md" />
      </a>

      <div class="min-w-0 flex-1">
        <Link
          to="/documents/$slug"
          params={{ slug: document().slug }}
          class="block truncate text-sm font-medium text-slate-900 dark:text-slate-100"
        >
          {document().title ?? document().slug}
        </Link>
        <p class="flex min-w-0 items-center gap-1.5 text-xs text-slate-500">
          <Show when={(properties.showProject ?? true) && document().project}>
            {project => (
              <>
                <Link
                  to="/projects/$project"
                  params={{ project: project() }}
                  class="flex shrink-0 items-center gap-1.5 text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
                >
                  <ProjectFavicon project={project()} class="size-4" />
                  {project()}
                </Link>
                <span aria-hidden="true">-</span>
              </>
            )}
          </Show>
          <span class="truncate font-mono">{document().slug}</span>
          <span aria-hidden="true">-</span>
          <span class="shrink-0">{`rev ${document().latest_revision}`}</span>
        </p>
      </div>

      <div class="flex shrink-0 items-center gap-3.5 text-xs text-slate-500">
        {/* On a phone the title needs the width more than these two do. */}
        <span class="hidden items-center gap-3.5 sm:flex">
          <Answered document={document()} />
          <Show when={document().tags[0]}>
            {tag => <TagMark tag={tag()} showLabel />}
          </Show>
        </span>
        <Visibility published={document().published} />
        <PushedAt at={document().last_pushed_at} class="w-14 text-right" />
      </div>
    </li>
  );
};
