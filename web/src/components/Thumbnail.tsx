import clsx from "clsx";
import { createSignal, onCleanup, onMount, Show } from "solid-js";

type Properties = {
  slug: string;
  /// Sizing and framing for the box; the image fills it.
  class?: string;
};

type ImageState = "loading" | "loaded" | "missing";

/// The drawn placeholder at rest, and the rendered screenshot while the
/// nearest `group` ancestor is hovered or holds focus. The screenshot is only
/// requested on the first hover or focus, so a list costs one small SVG per
/// document. Both come from one worker job per revision, so a 404 means the
/// worker has not reached this revision yet or is switched off; the skeleton
/// underneath covers that and the wait on a slow connection. A revision
/// rendered before placeholders existed has only its screenshot, which then
/// loads at once and shows at rest.
export const Thumbnail = (properties: Properties) => {
  const [placeholder, setPlaceholder] = createSignal<ImageState>("loading");
  const [screenshot, setScreenshot] = createSignal<ImageState>("loading");
  // a touch screen has no hover to wait for, so it shows the screenshot
  const [isWanted, setWanted] = createSignal(matchMedia("(hover: none)").matches);
  const want = () => setWanted(true);
  let frame: HTMLDivElement | undefined;

  onMount(() => {
    // the hover target is the whole row or card, not only the picture
    const host = frame?.closest(".group") ?? frame;

    if (host === undefined) return;

    host.addEventListener("pointerenter", want, { once: true });
    host.addEventListener("focusin", want, { once: true });
    onCleanup(() => {
      host.removeEventListener("pointerenter", want);
      host.removeEventListener("focusin", want);
    });
  });

  const source = (kind: "preview" | "placeholder", scheme?: "dark") => {
    const path = `/api/docs/${encodeURIComponent(properties.slug)}/${kind}`;

    return scheme === undefined ? path : `${path}?scheme=${scheme}`;
  };
  const screenshotOpacity = () => {
    if (screenshot() !== "loaded") return "opacity-0";

    if (placeholder() === "missing") return "opacity-100";

    return "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 [@media(hover:none)]:opacity-100";
  };

  return (
    <div
      ref={element => (frame = element)}
      class={clsx("thumb-skeleton relative shrink-0 overflow-hidden", properties.class)}
    >
      {/* Both schemes are captured per revision, so a reader in dark mode
          sees the document as they would open it. */}
      <Show when={placeholder() !== "missing"}>
        <picture class="contents">
          <source media="(prefers-color-scheme: dark)" srcset={source("placeholder", "dark")} />
          <img
            src={source("placeholder")}
            alt=""
            loading="lazy"
            onLoad={() => setPlaceholder("loaded")}
            onError={() => setPlaceholder("missing")}
            class={clsx(
              "absolute inset-0 z-10 size-full object-cover object-top",
              placeholder() === "loaded" ? "opacity-100" : "opacity-0",
            )}
          />
        </picture>
      </Show>
      <Show when={(isWanted() || placeholder() === "missing") && screenshot() !== "missing"}>
        <picture class="contents">
          <source media="(prefers-color-scheme: dark)" srcset={source("preview", "dark")} />
          <img
            src={source("preview")}
            alt=""
            loading="lazy"
            onLoad={() => setScreenshot("loaded")}
            onError={() => setScreenshot("missing")}
            class={clsx(
              "relative z-20 size-full bg-surface object-cover object-top",
              screenshotOpacity(),
            )}
          />
        </picture>
      </Show>
    </div>
  );
};
