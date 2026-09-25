import type { JSX } from "solid-js";
import { splitProps } from "solid-js";

import { FIELD, FIELD_LABEL } from "./Control";

type TextInputProperties = JSX.InputHTMLAttributes<HTMLInputElement> & {
  label: string;
};

export const TextInput = (properties: TextInputProperties) => {
  const [local, rest] = splitProps(properties, ["label"]);

  return (
    <label class="block space-y-1.5">
      <span class={FIELD_LABEL}>{local.label}</span>
      <input {...rest} class={FIELD} />
    </label>
  );
};
