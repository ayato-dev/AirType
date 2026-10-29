import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";

const page = (name: string) => fileURLToPath(new URL(name, import.meta.url));

export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: ["es2021", "chrome105", "safari15"],
    rollupOptions: {
      input: { main: page("index.html"), hud: page("hud.html"), panel: page("panel.html") },
    },
  },
});
