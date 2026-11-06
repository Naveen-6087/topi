# Website

The project's landing page: an [Astro](https://astro.build) site with no UI framework.

```bash
npm install
npm run dev      # http://localhost:4321
npm run build    # static output in dist/
```

## Design

Two grounds and two signals. Sections alternate between night (`#0e0d0c`) and paper (`#f4f3ee`); red is a
payment that can still be traced, amber a proof or anything that has left the mixnet or the pool.

- Headings are set with a few letters as pixel glyphs (`PixelText.astro`, `src/lib/glyphs.ts`); they decode
  out of pixel noise when first seen, and the hero's letters keep flickering into pixels now and then.
- The hero is one centred line over a sparse field of drifting packets (`src/scripts/drift.ts`).
- The mixnet section (`src/scripts/flow.ts`) is a box of three hops: packets enter in their senders'
  colours, are shuffled hop by hop with cover traffic, and leave identical.
- The pool section passes coins behind a glass shield; they vanish inside and come out as sealed notes.
- The network section is a halftone globe you can spin (`src/scripts/mixnet.ts`).
- The post-quantum section has isometric line drawings generated from geometry in `Quantum.astro`.
- The architecture starts as a closed block that opens as the section is scrolled through.
- The console section is a live replica of `topi` with install commands for each platform.
- Build is an editor whose code types itself in; code is highlighted by Shiki (`src/lib/code-theme.ts`).
- Sections meet at pixel-dither seams with packets sliding along them (`Divider.astro`).
- Readers who prefer reduced motion get the same animations, slower, and the architecture already open.
- Keyframe names are global across components, so each component prefixes its own where they could clash.

Security headers (a strict CSP, HSTS and the rest) are in `public/_headers`; the site
sets no cookies and has a privacy page, `robots.txt`, a sitemap and `/.well-known/security.txt`. See
[DEPLOYING.md](DEPLOYING.md) for hosting, DNS and DDoS protection.

Deployed on Cloudflare Pages: `npm run deploy` builds and uploads (after `npx wrangler login`), and
`.github/workflows/website.yml` deploys on pushes to `main` once `DEPLOY_WEBSITE` and the Cloudflare
secrets are set.
