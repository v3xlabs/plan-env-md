import type { JSX } from "solid-js";

/// One labelled group in a settings dialog. A dialog holds several unrelated
/// things, and without a heading each they read as one long column of controls.
export const SettingsSection = (properties: { title: string; children: JSX.Element; }) => (
  <section class="space-y-2">
    <h3 class="font-mono text-xs tracking-wide text-muted uppercase">{properties.title}</h3>
    {properties.children}
  </section>
);
