import { useQuery } from "@tanstack/solid-query";
import clsx from "clsx";
import { createSignal, Show } from "solid-js";

import { faviconUrl, projectsQueryOptions } from "../api/projects";
import { theme } from "../theme";
import { derivedColor, PROJECT_COLORS } from "./projectColor";

type Properties = {
  project: string;
  /// Pins one variant, for the settings slot that is about that variant.
  scheme?: "light" | "dark";
  class?: string;
};

/// The project's icon, falling back to its initial. Whether an icon exists
/// comes from the project list, so a project without one costs no request.
/// The server answers either scheme with the other when only one was uploaded.
export const ProjectFavicon = (properties: Properties) => {
  const projects = useQuery(() => projectsQueryOptions);
  const [isBroken, setBroken] = createSignal(false);

  const project = () => projects.data?.find(entry => entry.slug === properties.project);

  const hasIcon = () => {
    const found = project();

    if (found === undefined) return false;

    if (properties.scheme === "light") return found.has_favicon_light;

    if (properties.scheme === "dark") return found.has_favicon_dark;

    return found.has_favicon_light || found.has_favicon_dark;
  };

  return (
    <Show
      when={hasIcon() && !isBroken()}
      fallback={(
        <span
          class={clsx(
            "inline-flex shrink-0 items-center justify-center rounded text-[0.6em] font-semibold text-white",
            PROJECT_COLORS[project()?.color ?? derivedColor(properties.project)],
            properties.class,
          )}
        >
          {properties.project.slice(0, 1).toUpperCase()}
        </span>
      )}
    >
      <img
        src={faviconUrl(properties.project, properties.scheme ?? theme())}
        alt=""
        class={clsx("shrink-0 rounded object-contain", properties.class)}
        onError={() => setBroken(true)}
      />
    </Show>
  );
};
