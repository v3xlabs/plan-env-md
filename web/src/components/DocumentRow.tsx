import { Link } from "@tanstack/solid-router";
import { createSignal, For, Show } from "solid-js";

import type { DocumentSummary } from "../api/documents";
import { absolute, relative } from "../time";
import { iconForTag, LockIcon, PublishedIcon } from "./Icon";
import { ProjectFavicon } from "./ProjectFavicon";

/// The rendered thumbnail, or nothing. A 404 means the worker has not reached
/// this revision yet, or is switched off entirely, so a failed load is an
/// expected state rather than an error worth showing.
const Thumbnail = (properties: { slug: string; }) => {
  const [isMissing, setMissing] = createSignal(false);
  const preview = (scheme?: "dark") => {
    const path = `/api/docs/${encodeURIComponent(properties.slug)}/preview`;

    return scheme === undefined ? path : `${path}?scheme=${scheme}`;
  };

  return (
    <Show when={!isMissing()}>
      <a href={preview()} class="hidden shrink-0 sm:block">
        {/* Both schemes are rendered per revision, so a reader in dark mode
            sees the document as they would open it. */}
        <picture class="contents">
          <source media="(prefers-color-scheme: dark)" srcset={preview("dark")} />
          <img
            src={preview()}
            alt=""
            width="96"
            height="60"
            loading="lazy"
            class="h-15 w-24 rounded border border-line bg-surface object-cover object-top"
            onError={() => setMissing(true)}
          />
        </picture>
      </a>
    </Show>
  );
};

type Properties = {
  document: DocumentSummary;
  /// A project page already says which project it is, so the label is noise there
  showProject?: boolean;
};

export const DocumentRow = (properties: Properties) => {
  const document = () => properties.document;
  const unanswered = () => document().questions_total - document().questions_answered;

  return (
    <li class="flex items-start gap-3 py-3">
      <div class="flex w-10 shrink-0 justify-end gap-1 pt-1 text-base text-muted">
        <For each={document().tags.slice(0, 2)}>
          {(tag) => {
            const TagIcon = iconForTag(tag);

            return <TagIcon aria-label={tag} title={tag} />;
          }}
        </For>
      </div>

      <Thumbnail slug={document().slug} />

      <div class="min-w-0 flex-1">
        <Link
          to="/documents/$slug"
          params={{ slug: document().slug }}
          class="font-medium text-ink hover:text-accent"
        >
          {document().title ?? document().slug}
        </Link>
        <p class="flex min-w-0 items-center gap-1.5 font-mono text-xs text-muted">
          <Show when={(properties.showProject ?? true) && document().project}>
            {project => (
              <Link
                to="/projects/$project"
                params={{ project: project() }}
                class="flex shrink-0 items-center gap-1 text-ink hover:text-accent"
              >
                <ProjectFavicon project={project()} has class="size-3.5" />
                {project()}
              </Link>
            )}
          </Show>
          <span class="truncate">
            {document().slug}
            {" - "}
            rev
            {" "}
            {document().latest_revision}
            <Show when={document().questions_total > 0}>
              {" - "}
              <span class={unanswered() > 0 ? "text-accent" : undefined}>
                {document().questions_answered}
                {" of "}
                {document().questions_total}
                {" answered"}
              </span>
            </Show>
          </span>
        </p>
      </div>

      <div class="w-36 shrink-0 text-right">
        <time datetime={document().last_pushed_at} class="block text-xs text-ink">
          {absolute(document().last_pushed_at)}
        </time>
        <span class="text-xs text-muted">{relative(document().last_pushed_at)}</span>
      </div>

      <a
        href={document().url}
        class="mt-0.5 shrink-0 text-base text-muted hover:text-ink"
        title={document().published ? "Published" : "Private"}
      >
        <Show when={document().published} fallback={<LockIcon aria-label="Private" />}>
          <PublishedIcon aria-label="Published" />
        </Show>
      </a>
    </li>
  );
};
