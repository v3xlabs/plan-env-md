import type { IconTypes } from "solid-icons";
import { For, Show } from "solid-js";
import { Dynamic } from "solid-js/web";

import { FOCUS } from "./Control";

export type SegmentedOption<T extends string> = {
  value: T;
  label: string;
  /// Shown alone when present, with the label as its accessible name.
  icon?: IconTypes;
};

type Properties<T extends string> = {
  label: string;
  options: readonly SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
};

/// error-menu's segmented control: no track, only the chosen option is filled.
/// A filled button inside a filled track would be a panel inside a panel.
const SEGMENT = `flex h-8 items-center rounded-control text-sm text-slate-500 hover:text-slate-900 aria-pressed:bg-raised aria-pressed:font-medium aria-pressed:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100 dark:aria-pressed:text-slate-100 ${FOCUS}`;
const SEGMENT_TEXT = `${SEGMENT} px-2.5`;
const SEGMENT_ICON = `${SEGMENT} w-8 justify-center`;

export const Segmented = <T extends string>(properties: Properties<T>) => (
  <div role="group" aria-label={properties.label} class="flex shrink-0 gap-0.5">
    <For each={properties.options}>
      {option => (
        <button
          type="button"
          aria-pressed={properties.value === option.value}
          aria-label={option.icon ? option.label : undefined}
          title={option.icon ? option.label : undefined}
          onClick={() => properties.onChange(option.value)}
          class={option.icon ? SEGMENT_ICON : SEGMENT_TEXT}
        >
          <Show when={option.icon} fallback={option.label}>
            {Icon => <Dynamic component={Icon()} size={16} aria-hidden="true" />}
          </Show>
        </button>
      )}
    </For>
  </div>
);
