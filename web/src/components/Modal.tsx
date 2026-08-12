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
      <Dialog.Overlay class="fixed inset-0 bg-black/40" />
      <div class="fixed inset-0 grid place-items-center p-4">
        <Dialog.Content class="w-full max-w-md rounded-lg border border-line bg-surface p-6 shadow-lg">
          <Dialog.Title class="mb-4 text-lg font-semibold">{properties.title}</Dialog.Title>
          {properties.children}
        </Dialog.Content>
      </div>
    </Dialog.Portal>
  </Dialog>
);
