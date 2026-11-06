import type { APIRoute } from "astro";

const PAGES = ["", "privacy"];

export const GET: APIRoute = ({ site }) => {
  const urls = PAGES.map((p) => `  <url><loc>${new URL(p, site)}</loc></url>`).join("\n");
  return new Response(`<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls}\n</urlset>\n`, {
    headers: { "content-type": "application/xml; charset=utf-8" },
  });
};
