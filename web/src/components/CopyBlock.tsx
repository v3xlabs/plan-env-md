import { createSignal } from "solid-js";

import { Button } from "./Button";

type CopyBlockProperties = {
  text: string;
};

export const CopyBlock = (properties: CopyBlockProperties) => {
  const [hasCopied, setHasCopied] = createSignal(false);

  return (
    <div class="flex items-start gap-2">
      <pre class="min-w-0 flex-1 overflow-x-auto rounded-md border border-line bg-bg p-3 font-mono text-xs leading-relaxed">
        {properties.text}
      </pre>
      <Button
        variant="quiet"
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
