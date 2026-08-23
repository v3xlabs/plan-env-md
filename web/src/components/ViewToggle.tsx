import clsx from "clsx";
import type { IconTypes } from "solid-icons";
import {
  TbOutlineFolders,
  TbOutlineLayoutColumns,
  TbOutlineLayoutGrid,
  TbOutlineList,
} from "solid-icons/tb";
import { For } from "solid-js";

/// How a page lays its documents out. `projects` also changes what the
/// headings group by, so it is offered only where more than one project can
/// turn up.
const VIEWS = ["list", "cards", "tiles", "projects"] as const;

export type View = typeof VIEWS[number];

const APPEARANCE: Record<View, { icon: IconTypes; label: string; }> = {
  list: { icon: TbOutlineList, label: "List" },
  cards: { icon: TbOutlineLayoutGrid, label: "Cards" },
  tiles: { icon: TbOutlineLayoutColumns, label: "Two columns" },
  projects: { icon: TbOutlineFolders, label: "By project" },
};

export const parseView = (value: unknown): View | undefined =>
  VIEWS.find(view => view === value);

type Properties = {
  value: View;
  /// Which views this page offers, in the order they are shown.
  views: readonly View[];
  onChange: (view: View) => void;
};

export const ViewToggle = (properties: Properties) => (
  <div class="flex shrink-0 divide-x divide-line overflow-hidden rounded-md border border-line">
    <For each={properties.views}>
      {(view) => {
        const { icon: ViewIcon, label } = APPEARANCE[view];

        return (
          <button
            type="button"
            onClick={() => properties.onChange(view)}
            title={label}
            aria-label={label}
            aria-pressed={properties.value === view}
            class={clsx(
              "px-2 py-1.5 hover:text-ink",
              properties.value === view ? "bg-surface text-ink" : "text-muted",
            )}
          >
            <ViewIcon class="size-4" aria-hidden="true" />
          </button>
        );
      }}
    </For>
  </div>
);
