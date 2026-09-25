import clsx from "clsx";
import type { JSX } from "solid-js";
import { splitProps } from "solid-js";

import { BUTTON_DANGER, BUTTON_PRIMARY, BUTTON_SECONDARY } from "./Control";

type ButtonProperties = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "danger";
};

const VARIANTS = {
  primary: BUTTON_PRIMARY,
  secondary: BUTTON_SECONDARY,
  danger: BUTTON_DANGER,
};

export const Button = (properties: ButtonProperties) => {
  const [local, rest] = splitProps(properties, ["variant", "class"]);

  return (
    <button
      type="button"
      {...rest}
      class={clsx(VARIANTS[local.variant ?? "primary"], local.class)}
    />
  );
};
