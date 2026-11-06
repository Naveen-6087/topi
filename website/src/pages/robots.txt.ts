import type { APIRoute } from "astro";

/** Everything may be crawled; the sitemap lists the pages. */
export const GET: APIRoute = ({ site }) =>
  new Response(`User-agent: *\nAllow: /\n\nSitemap: ${new URL("sitemap.xml", site)}\n`, {
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
