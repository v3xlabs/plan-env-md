import clsx from "clsx";
import { createSignal, Show } from "solid-js";

type Properties = {
  slug: string;
  /// Sizing and framing for the box; the image fills it.
  class?: string;
};

/// The rendered preview, over a drawn page that stands in for one. The worker
/// renders a preview per revision, so a document is usually looked at before
/// its picture exists, and a 404 means the worker has not reached this
/// revision yet or is switched off entirely. The drawn page covers both, and
/// covers the wait on a slow connection.
export const Thumbnail = (properties: Properties) => {
  const [isMissing, setMissing] = createSignal(false);
  const [isLoaded, setLoaded] = createSignal(false);
  const preview = (scheme?: "dark") => {
    const path = `/api/docs/${encodeURIComponent(properties.slug)}/preview`;

    return scheme === undefined ? path : `${path}?scheme=${scheme}`;
  };

  return (
    <div class={clsx("thumb-skeleton relative shrink-0 overflow-hidden", properties.class)}>
      <Show when={!isMissing()}>
        {/* Both schemes are rendered per revision, so a reader in dark mode
            sees the document as they would open it. */}
        <picture class="contents">
          <source media="(prefers-color-scheme: dark)" srcset={preview("dark")} />
          <img
            src={preview()}
            alt=""
            loading="lazy"
            onLoad={() => setLoaded(true)}
            onError={() => setMissing(true)}
            class={clsx(
              "relative size-full bg-surface object-cover object-top",
              "transition-opacity duration-200",
              isLoaded() ? "opacity-100" : "opacity-0",
            )}
          />
        </picture>
      </Show>
    </div>
  );
};
