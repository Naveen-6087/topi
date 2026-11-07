import { defineConfig } from "astro/config";

// The site's address: canonical links, the sitemap and security.txt are built from it.
// Change it here when the site moves to its own domain.
export default defineConfig({
  site: "https://usetopi.pages.dev",
  build: { inlineStylesheets: "auto" },
  // Every script is emitted as a file, so the content security policy can allow scripts from 'self' only.
  vite: { build: { assetsInlineLimit: 0 } },
});
