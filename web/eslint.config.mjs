import v3xlabs from "eslint-plugin-v3xlabs";

// eslint-disable-next-line import/no-default-export -- ESLint flat config requires a default export
export default [
  { ignores: ["dist", "src/routeTree.gen.ts", "src/api/schema.gen.ts"] },
  ...v3xlabs.configs.recommended,
  ...v3xlabs.configs.solid,
  ...v3xlabs.configs.tailwindcss,
];
