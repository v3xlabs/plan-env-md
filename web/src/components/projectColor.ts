import type { ProjectColor } from "../api/projects";

/// Same order as the server's closed set.
export const PALETTE: readonly ProjectColor[] = [
  "red",
  "orange",
  "amber",
  "emerald",
  "teal",
  "sky",
  "indigo",
  "pink",
];

/// Class names are spelled out in full so Tailwind's scanner finds them.
export const PROJECT_COLORS: Record<ProjectColor, string> = {
  red: "bg-red-500",
  orange: "bg-orange-500",
  amber: "bg-amber-500",
  emerald: "bg-emerald-500",
  teal: "bg-teal-500",
  sky: "bg-sky-500",
  indigo: "bg-indigo-500",
  pink: "bg-pink-500",
};

/// The colour a project shows until someone picks one: a stable hash of the
/// slug, so the same project keeps the same colour on every page and reload.
export const derivedColor = (slug: string): ProjectColor => {
  let hash = 0;

  for (const character of slug) hash = (hash * 31 + (character.codePointAt(0) ?? 0)) % 2_147_483_647;

  return PALETTE[hash % PALETTE.length] ?? "sky";
};
