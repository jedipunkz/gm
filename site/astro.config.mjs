// @ts-check
import { defineConfig } from "astro/config";

// Served from https://jedipunkz.github.io/gm/, a project page, so every
// link has to carry the /gm prefix. SITE and BASE override both for a fork
// or a custom domain.
export default defineConfig({
  site: process.env.SITE ?? "https://jedipunkz.github.io",
  base: process.env.BASE ?? "/gm",
  trailingSlash: "ignore",
});
