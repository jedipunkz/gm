// @ts-check
import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";

// Served from https://jedipunkz.github.io/gm/, a project page, so every
// link has to carry the /gm prefix. SITE and BASE override both for a fork
// or a custom domain.
export default defineConfig({
  site: process.env.SITE ?? "https://jedipunkz.github.io",
  base: process.env.BASE ?? "/gm",
  trailingSlash: "ignore",
  // A sitemap for Search Console, generated from site and base, so a fork
  // that overrides either gets its own URLs rather than these.
  //
  // trailingSlash "ignore" makes Astro offer /gm and /gm/ as separate routes
  // for the one page. Only the slashed form is the canonical the page
  // declares, and listing both would be a duplicate to index.
  integrations: [sitemap({ filter: (page) => page.endsWith("/") })],
});
