import { createSignal, Show } from "solid-js";

import { faviconUrl } from "../api/projects";

type Properties = {
  project: string;
  has: boolean;
  scheme?: "light" | "dark";
  class?: string;
};

/// The project's icon, falling back to its initial. The endpoint is owner only
/// and returns 404 when nothing was uploaded, so a failed load is expected
/// rather than an error worth reporting.
export const ProjectFavicon = (properties: Properties) => {
  const [isBroken, setBroken] = createSignal(false);

  return (
    <Show
      when={properties.has && !isBroken()}
      fallback={(
        <span
          class={properties.class}
          classList={{
            "inline-flex items-center justify-center rounded bg-line font-mono text-[0.6em] text-muted": true,
          }}
        >
          {properties.project.slice(0, 1).toUpperCase()}
        </span>
      )}
    >
      {/* A named scheme feeds both candidates, so the settings page keeps
          showing the variant it is about while everywhere else follows the
          reader. `contents` keeps the image itself as the layout box. */}
      <picture class="contents">
        <source
          media="(prefers-color-scheme: dark)"
          srcset={faviconUrl(properties.project, properties.scheme ?? "dark")}
        />
        <img
          src={faviconUrl(properties.project, properties.scheme ?? "light")}
          alt=""
          class={properties.class}
          classList={{ "rounded object-contain": true }}
          onError={() => setBroken(true)}
        />
      </picture>
    </Show>
  );
};
