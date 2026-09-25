import { TbOutlineMoon, TbOutlineSun } from "solid-icons/tb";
import { Show } from "solid-js";

import { theme, toggleTheme } from "../theme";
import { GLYPH_RAISED } from "./Control";

export const ThemeToggle = () => (
  <button
    type="button"
    onClick={toggleTheme}
    class={GLYPH_RAISED}
    title={theme() === "dark" ? "Switch to light" : "Switch to dark"}
    aria-label={theme() === "dark" ? "Switch to light" : "Switch to dark"}
  >
    <Show when={theme() === "dark"} fallback={<TbOutlineMoon size={16} aria-hidden="true" />}>
      <TbOutlineSun size={16} aria-hidden="true" />
    </Show>
  </button>
);
