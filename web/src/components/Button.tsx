import clsx from "clsx";
import type { JSX } from "solid-js";
import { splitProps } from "solid-js";

type ButtonProperties = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "quiet" | "danger";
};

export const Button = (properties: ButtonProperties) => {
  const [local, rest] = splitProps(properties, ["variant", "class"]);

  return (
    <button
      type="button"
      {...rest}
      class={clsx(
        "rounded-md px-3 py-1.5 text-sm font-medium transition-colors",
        "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
        "disabled:cursor-not-allowed disabled:opacity-50",
        (local.variant ?? "primary") === "primary"
        && "bg-accent text-accent-contrast hover:opacity-90",
        local.variant === "quiet" && "border border-line bg-surface text-ink hover:bg-bg",
        local.variant === "danger"
        && "border border-line bg-surface text-red-700 hover:bg-bg dark:text-red-400",
        local.class,
      )}
    />
  );
};
