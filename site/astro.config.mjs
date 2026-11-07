import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import { BASE, REPO_URL, SITE } from "./docs.config.mjs";
import { generate } from "./scripts/generate-docs.mjs";

// The pages are written here, before Astro reads the content folder, so `astro dev` and
// `astro build` both start from the current docs. A broken link in them throws out of this line.
const { sidebar, watch } = generate();

/** In `astro dev`, editing a source document regenerates its pages. */
const regenerate = {
  name: "marbble-docs",
  hooks: {
    "astro:server:setup": ({ server }) => {
      server.watcher.add(watch);
      server.watcher.on("change", (file) => {
        if (watch.includes(file)) generate();
      });
    },
  },
};

export default defineConfig({
  site: SITE,
  base: BASE || "/",
  trailingSlash: "always",
  integrations: [
    regenerate,
    starlight({
      title: "Marbble",
      description: "User guide and developer documentation for Marbble, an open-source digital audio workstation written in Rust.",
      favicon: "/favicon.svg",
      social: [{ icon: "github", label: "GitHub", href: REPO_URL }],
      sidebar,
      tableOfContents: { minHeadingLevel: 2, maxHeadingLevel: 3 },
      editLink: { baseUrl: `${REPO_URL}/edit/main/` },
      customCss: [
        "@fontsource-variable/archivo/wght.css",
        "@fontsource-variable/jetbrains-mono/wght.css",
        "./src/styles/docs.css",
      ],
      routeMiddleware: "./src/route-data.ts",
      components: {
        Sidebar: "./src/components/Sidebar.astro",
        SiteTitle: "./src/components/SiteTitle.astro",
        Head: "./src/components/Head.astro",
        Footer: "./src/components/Footer.astro",
      },
      expressiveCode: { themes: ["github-dark", "github-light"], styleOverrides: { borderRadius: "4px" } },
      head: [
        { tag: "meta", attrs: { property: "og:image", content: `${SITE}${BASE}/og.png` } },
      ],
    }),
  ],
});
