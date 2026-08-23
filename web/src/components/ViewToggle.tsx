import clsx from "clsx";
import type { IconTypes } from "solid-icons";
import { TbOutlineLayoutColumns, TbOutlineLayoutGrid, TbOutlineList } from "solid-icons/tb";
import { For } from "solid-js";

/// How a page lays its documents out.
const VIEWS = ["list", "tiles", "cards"] as const;

export type View = typeof VIEWS[number];

const APPEARANCE: Record<View, { icon: IconTypes; label: string; }> = {
  list: { icon: TbOutlineList, label: "List" },
  tiles: { icon: TbOutlineLayoutColumns, label: "Two columns" },
  cards: { icon: TbOutlineLayoutGrid, label: "Cards" },
};

export const parseView = (value: unknown): View | undefined =>
  VIEWS.find(view => view === value);

type Properties = {
  value: View;
  onChange: (view: View) => void;
};

export const ViewToggle = (properties: Properties) => (
  <div class="flex shrink-0 divide-x divide-line overflow-hidden rounded-md border border-line">
    <For each={VIEWS}>
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
