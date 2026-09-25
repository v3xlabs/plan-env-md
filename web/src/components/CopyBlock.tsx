import { createSignal } from "solid-js";

import { Button } from "./Button";

type CopyBlockProperties = {
  text: string;
};

export const CopyBlock = (properties: CopyBlockProperties) => {
  const [hasCopied, setHasCopied] = createSignal(false);

  return (
    <div class="flex items-start gap-2">
      <pre class="min-w-0 flex-1 overflow-x-auto rounded-control bg-raised p-3 text-left font-mono text-xs/relaxed text-slate-700 dark:text-slate-300">
        {properties.text}
      </pre>
      <Button
        variant="secondary"
        onClick={() => {
          void navigator.clipboard.writeText(properties.text).then(() => {
            setHasCopied(true);
            setTimeout(() => setHasCopied(false), 1500);
          });
        }}
      >
        {hasCopied() ? "Copied" : "Copy"}
      </Button>
    </div>
  );
};
