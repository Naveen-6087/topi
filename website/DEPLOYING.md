# Deploying the website

The site is static (`npm run build` writes `dist/`), so any static host works. What the host must do is serve
the security headers in `public/_headers`, terminate TLS, and absorb abusive traffic.

## Cloudflare Pages (current), all on the free plan

| Need | How it is met |
|---|---|
| Hosting and CDN | Cloudflare Pages, free tier: the build output is served from Cloudflare's edge in every region |
| Security headers | `public/_headers` is read by Pages as is (CSP, HSTS, framing, referrer, permissions) |
| DDoS protection | Unmetered network-layer and HTTP DDoS mitigation on every plan, on by default |
| Origin IP | There is none to leak: Pages has no origin server |
| TLS | Automatic certificates; set SSL to Full (strict), minimum TLS 1.2, Always Use HTTPS |
| Bots and abuse | Bot Fight Mode, and a free WAF rate-limiting rule (for example 100 requests per 10 s per IP) |
| DNS | Cloudflare DNS with DNSSEC on, and a CAA record naming only the certificate authorities Cloudflare uses |
| Registrar | Cloudflare Registrar sells domains at cost, with no renewal mark-up and free WHOIS privacy |

Deploy by hand from `website/`:

```bash
npx wrangler login      # once
npm run deploy          # build, then upload dist/ to the Pages project "topi"
```

Or from CI: set the repository variable `DEPLOY_WEBSITE=true` and the secrets `CLOUDFLARE_API_TOKEN`
(a token with Cloudflare Pages: Edit) and `CLOUDFLARE_ACCOUNT_ID`; `.github/workflows/website.yml` then
deploys every push to `main` that touches `website/`.

When a domain is bought:

1. In the Pages project, add the custom domain; Pages creates the DNS records and the certificate.
2. In the zone: DNSSEC on; SSL Full (strict); minimum TLS 1.2; Always Use HTTPS; HSTS is already sent by
   `_headers`, so leave Cloudflare's HSTS setting off to avoid two policies; Bot Fight Mode on.
3. Set `site` in `astro.config.mjs` to the new address, so canonical links, the sitemap and
   `security.txt` name it. Once the domain is settled, consider adding `preload` to the HSTS header and
   submitting it to the HSTS preload list.

## Choosing a domain

At-cost registrars (Cloudflare, Porkbun) keep renewals at the first year's price, which matters more than an
introductory discount. Check current prices before buying; in recent years a `.com`, `.org` or `.dev` has
cost roughly 8 to 12 US dollars a year at cost, while `.xyz` and similar extensions are often cheaper in the
first year and more expensive to renew. `.dev` and `.app` are on the HSTS preload list, so they are HTTPS
only by design.

## Other hosts

- **Netlify**: reads `public/_headers` as is.
- **GitHub Pages**: free, but cannot send custom headers, so the CSP and HSTS here would be lost; not
  recommended.

## What the site does not need

No server, no database, no cookies, no analytics. The only request it makes to another origin is to the
testnet RPC for the latest block, which the CSP's `connect-src` allows by name.
