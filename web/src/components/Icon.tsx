import type { IconTypes } from "solid-icons";
import {
  TbOutlineActivity,
  TbOutlineBook,
  TbOutlineColumns,
  TbOutlineFileSearch,
  TbOutlineFileText,
  TbOutlineLayout,
  TbOutlineMap,
  TbOutlineMicroscope,
  TbOutlineRoute,
  TbOutlineShieldCheck,
  TbOutlineShieldLock,
  TbOutlineTag,
} from "solid-icons/tb";

// Tags are a living vocabulary: the server never checks one against a list, so
// an unrecognised tag has to render correctly on the day it is invented.
const TAG_ICONS: Record<string, IconTypes> = {
  "plan": TbOutlineMap,
  "roadmap": TbOutlineRoute,
  "review": TbOutlineFileSearch,
  "pr-review": TbOutlineFileSearch,
  "audit": TbOutlineShieldCheck,
  "security": TbOutlineShieldLock,
  "spec": TbOutlineFileText,
  "status": TbOutlineActivity,
  "explainer": TbOutlineBook,
  "comparison": TbOutlineColumns,
  "mockup": TbOutlineLayout,
  "research": TbOutlineMicroscope,
};

export const iconForTag = (tag: string): IconTypes => TAG_ICONS[tag] ?? TbOutlineTag;

export { TbOutlineLock as LockIcon, TbOutlineWorld as PublishedIcon } from "solid-icons/tb";
