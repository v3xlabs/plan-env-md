import clsx from "clsx";
import { Show } from "solid-js";

import type { DocumentSummary } from "../api/documents";
import { absolute, compactRelative } from "../time";
import { iconForTag, LockIcon, PublishedIcon } from "./Icon";

/// Published is the only state that reaches outside the account, so it is the
/// one that carries a colour.
export const Visibility = (properties: { published: boolean; }) => (
  <Show
    when={properties.published}
    fallback={(
      <LockIcon
        size={15}
        aria-label="Private"
        title="Private"
        class="shrink-0"
      />
    )}
  >
    <PublishedIcon
      size={15}
      aria-label="Published"
      title="Published"
      class="shrink-0 text-emerald-600 dark:text-emerald-400"
    />
  </Show>
);

export const TagMark = (properties: { tag: string; showLabel?: boolean; }) => {
  const TagIcon = iconForTag(properties.tag);

  return (
    <span class="flex shrink-0 items-center gap-1" title={properties.tag}>
      <TagIcon size={15} aria-hidden={properties.showLabel} aria-label={properties.showLabel ? undefined : properties.tag} />
      <Show when={properties.showLabel}>{properties.tag}</Show>
    </span>
  );
};

/// Nothing when a document asks nothing. Open questions carry amber, the status
/// colour both sibling apps use for "needs attention".
export const Answered = (properties: { document: DocumentSummary; }) => {
  const open = () => properties.document.questions_total - properties.document.questions_answered;

  return (
    <Show when={properties.document.questions_total > 0}>
      <span
        title={`${properties.document.questions_answered} of ${properties.document.questions_total} answered`}
        class={clsx(
          "shrink-0 tabular-nums",
          open() > 0 && "font-medium text-amber-700 dark:text-amber-400",
        )}
      >
        {properties.document.questions_answered}
        /
        {properties.document.questions_total}
      </span>
    </Show>
  );
};

export const PushedAt = (properties: { at: string; class?: string; }) => (
  <time datetime={properties.at} title={absolute(properties.at)} class={clsx("shrink-0 tabular-nums", properties.class)}>
    {compactRelative(properties.at)}
  </time>
);
