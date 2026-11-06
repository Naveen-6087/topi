import type { APIRoute } from "astro";

const REPO = "https://github.com/Naveen-6087/topi";

/** RFC 9116. The expiry is set a year from each build, so a deployed site never carries a stale one. */
export const GET: APIRoute = ({ site }) => {
  const expires = new Date(Date.now() + 365 * 24 * 3600 * 1000).toISOString().replace(/\.\d+Z$/, "Z");
  const body = [
    `Contact: ${REPO}/security/advisories/new`,
    `Expires: ${expires}`,
    `Policy: ${REPO}/blob/main/SECURITY.md`,
    "Preferred-Languages: en",
    `Canonical: ${new URL(".well-known/security.txt", site)}`,
    "",
  ].join("\n");
  return new Response(body, { headers: { "content-type": "text/plain; charset=utf-8" } });
};
