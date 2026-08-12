import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [
    tanstackRouter({ target: "solid", autoCodeSplitting: true }),
    solid(),
    tailwindcss(),
  ],
  server: {
    proxy: {
      "/api": "http://127.0.0.1:3000",
    },
  },
});
