import { TbOutlineLayoutColumns, TbOutlineLayoutGrid, TbOutlineList } from "solid-icons/tb";
import { createSignal } from "solid-js";

import { Segmented, type SegmentedOption } from "./Segmented";

/// How a page lays its documents out.
const VIEW_OPTIONS = [
  { value: "list", label: "List", icon: TbOutlineList },
  { value: "tiles", label: "Two columns", icon: TbOutlineLayoutColumns },
  { value: "cards", label: "Cards", icon: TbOutlineLayoutGrid },
] as const satisfies readonly SegmentedOption<string>[];

export type View = typeof VIEW_OPTIONS[number]["value"];

const STORAGE_KEY = "plan-env-md-view";

// One preference for every page that lists documents: a reader who likes
// cards likes them on a project page too.
const [preferredView, setView] = createSignal<View>(
  VIEW_OPTIONS.find(option => option.value === localStorage.getItem(STORAGE_KEY))?.value ?? "list",
);

export { preferredView };

const choose = (view: View) => {
  localStorage.setItem(STORAGE_KEY, view);
  setView(view);
};

export const ViewToggle = () => (
  <Segmented
    label="Layout"
    options={VIEW_OPTIONS}
    value={preferredView()}
    onChange={choose}
  />
);
