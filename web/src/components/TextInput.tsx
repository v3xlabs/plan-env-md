import type { JSX } from "solid-js";
import { splitProps } from "solid-js";

type TextInputProperties = JSX.InputHTMLAttributes<HTMLInputElement> & {
  label: string;
};

export const TextInput = (properties: TextInputProperties) => {
  const [local, rest] = splitProps(properties, ["label"]);

  return (
    <label class="block">
      <span class="mb-1 block text-sm font-medium text-muted">{local.label}</span>
      <input
        {...rest}
        class={[
          "w-full rounded-md border border-line bg-surface px-3 py-1.5 text-ink",
          "focus:outline-2 focus:outline-offset-1 focus:outline-accent",
        ].join(" ")}
      />
    </label>
  );
};
