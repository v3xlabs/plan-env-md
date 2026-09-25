import { Dialog } from "@kobalte/core/dialog";
import type { JSX } from "solid-js";

type ModalProperties = {
  title: string;
  isOpen: boolean;
  onOpenChange: (isOpen: boolean) => void;
  children: JSX.Element;
};

export const Modal = (properties: ModalProperties) => (
  <Dialog open={properties.isOpen} onOpenChange={properties.onOpenChange}>
    <Dialog.Portal>
      <Dialog.Overlay class="fixed inset-0 z-40 bg-slate-950/40" />
      <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
        <Dialog.Content class="max-h-[85vh] w-full max-w-md space-y-4 overflow-y-auto rounded-panel bg-surface p-5 shadow-xl ring-1 ring-hairline">
          <Dialog.Title class="text-base font-semibold">{properties.title}</Dialog.Title>
          {properties.children}
        </Dialog.Content>
      </div>
    </Dialog.Portal>
  </Dialog>
);
