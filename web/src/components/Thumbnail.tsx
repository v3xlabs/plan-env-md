import clsx from "clsx";
import { createSignal, Show } from "solid-js";

type Properties = {
  slug: string;
  /// Where the picture leads. It is a picture of the plan, so it leads to the
  /// plan rather than to the image file it happens to be.
  href: string;
  /// Sizing for the link box; the image fills it.
  class?: string;
};

/// The rendered thumbnail, or nothing. A 404 means the worker has not reached
/// this revision yet, or is switched off entirely, so a failed load is an
/// expected state rather than an error worth showing.
export const Thumbnail = (properties: Properties) => {
  const [isMissing, setMissing] = createSignal(false);
  const preview = (scheme?: "dark") => {
    const path = `/api/docs/${encodeURIComponent(properties.slug)}/preview`;

    return scheme === undefined ? path : `${path}?scheme=${scheme}`;
  };

  return (
    <Show when={!isMissing()}>
      <a href={properties.href} class={clsx("block shrink-0", properties.class)}>
        {/* Both schemes are rendered per revision, so a reader in dark mode
            sees the document as they would open it. */}
        <picture class="contents">
          <source media="(prefers-color-scheme: dark)" srcset={preview("dark")} />
          <img
            src={preview()}
            alt=""
            loading="lazy"
            class="size-full rounded border border-line bg-surface object-cover object-top"
            onError={() => setMissing(true)}
          />
        </picture>
      </a>
    </Show>
  );
};
