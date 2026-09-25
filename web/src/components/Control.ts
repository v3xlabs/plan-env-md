// Class strings shared with error-menu and metered-usage, kept in one place the
// way metered-usage's Control.tsx does, so a toolbar or a dialog here reads the
// same as one there.

export const FOCUS = "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-500";

export const BUTTON_PRIMARY = `rounded-control bg-slate-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-slate-700 disabled:opacity-60 dark:bg-slate-100 dark:text-slate-900 dark:hover:bg-white ${FOCUS}`;
export const BUTTON_SECONDARY = `rounded-control bg-raised px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-raised-hover disabled:opacity-60 dark:text-slate-300 ${FOCUS}`;
export const BUTTON_DANGER = `rounded-control bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-60 ${FOCUS}`;

/// Every trigger and segmented group in a toolbar shares one height, so a row
/// lines up whatever mix of controls it holds.
export const TRIGGER = `flex h-8 items-center gap-1.5 rounded-control bg-raised px-2.5 text-sm font-medium whitespace-nowrap text-slate-700 hover:bg-raised-hover data-expanded:bg-raised-hover dark:text-slate-300 ${FOCUS}`;
export const GLYPH = `flex size-8 items-center justify-center rounded-control text-slate-500 hover:bg-raised hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100 ${FOCUS}`;
export const GLYPH_RAISED = `flex size-8 items-center justify-center rounded-control bg-raised text-slate-700 hover:bg-raised-hover dark:text-slate-300 ${FOCUS}`;

export const POPOVER = "z-50 rounded-panel bg-surface p-1.5 shadow-lg ring-1 ring-hairline outline-none";
const MENU_ITEM_BASE = "flex items-center gap-2 rounded-control px-2.5 py-1.5 text-sm no-underline outline-none select-none data-highlighted:bg-raised";

export const MENU_ITEM = `${MENU_ITEM_BASE} text-slate-700 dark:text-slate-300`;
export const MENU_ITEM_DANGER = `${MENU_ITEM_BASE} text-red-600 dark:text-red-400`;

export const FIELD = `w-full rounded-control bg-raised px-3 py-1.5 text-sm text-slate-900 placeholder:text-slate-500 dark:text-slate-100 ${FOCUS}`;
export const FIELD_LABEL = "block text-sm font-medium text-slate-700 dark:text-slate-300";

export const LIST = "divide-y divide-hairline overflow-hidden rounded-panel bg-surface";
export const EMPTY = "rounded-panel bg-surface px-4 py-8 text-center text-sm text-slate-500";
export const SECTION_TITLE = "text-sm font-semibold text-slate-700 dark:text-slate-300";
export const PAGE_TITLE = "text-lg font-semibold";
export const STATUS = "text-sm text-slate-500";
export const ERROR = "text-sm text-red-600 dark:text-red-400";
export const LINK = "font-medium text-slate-900 underline-offset-2 hover:underline dark:text-slate-100";
