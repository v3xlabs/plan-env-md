import { createSignal } from "solid-js";

export type Theme = "light" | "dark";

const STORAGE_KEY = "plan-env-md-theme";

// index.html sets the class before the first paint, so the class is the truth
// at startup and this only has to follow it
const [theme, setTheme] = createSignal<Theme>(
  document.documentElement.classList.contains("dark") ? "dark" : "light",
);

export { theme };

export const toggleTheme = () => {
  const next = theme() === "dark" ? "light" : "dark";

  localStorage.setItem(STORAGE_KEY, next);
  document.documentElement.classList.toggle("dark", next === "dark");
  setTheme(next);
};
