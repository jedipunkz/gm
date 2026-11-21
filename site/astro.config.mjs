// @ts-check
import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";

// Served from https://jedipunkz.rocks/gm/, a project page behind the account's
// custom domain, so every link has to carry the /gm prefix. The github.io
// address 301s here, so it is not the host to publish: a canonical, an og:url
// or a sitemap entry pointing at it names a URL that redirects away, and
// Search Console rejects a sitemap whose URLs are on another host.
export default defineConfig({
  site: process.env.SITE ?? "https://jedipunkz.rocks",
  base: process.env.BASE ?? "/gm",
  trailingSlash: "ignore",
  // trailingSlash "ignore" makes Astro offer /gm and /gm/ as separate routes
  // for the one page. Only the slashed form is the canonical the page
  // declares, and listing both would be a duplicate to index.
  integrations: [sitemap({ filter: (page) => page.endsWith("/") })],
});
