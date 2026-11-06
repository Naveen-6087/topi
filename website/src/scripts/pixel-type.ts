/**
 * Pixel type: headings decode out of pixel noise the first time they are seen, and live headings keep
 * flickering one letter at a time into its pixel glyph. The noise is drawn as a background on the letter,
 * with the letter itself made transparent, so nothing moves in the layout.
 */
import { GLYPHS } from "../lib/glyphs";

const svg = (rows: string[], color: string) => {
  const rects = rows
    .flatMap((row, y) => [...row].map((c, x) => (c === "#" ? `<rect x='${x}' y='${y}' width='1.02' height='1.02'/>` : "")))
    .join("");
  return `url("data:image/svg+xml,${encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 5 9' fill='${color}'>${rects}</svg>`)}")`;
};
const noise = (color: string) => {
  const rows = Array.from({ length: 9 }, (_, y) =>
    Array.from({ length: 5 }, () => (y > 1 && y < 7 ? Math.random() < 0.5 : Math.random() < 0.15) ? "#" : ".").join(""),
  );
  return svg(rows, color);
};

let mounted = false;

export function mountPixelType(): void {
  if (mounted) return;
  mounted = true;
  const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const decode = (el: HTMLElement) => {
    const color = getComputedStyle(el).color;
    const chars = [...el.querySelectorAll<HTMLElement>(".ch")];
    const pool = Array.from({ length: 8 }, () => noise(color));
    chars.forEach((c, i) => {
      c.classList.add("scr");
      c.style.backgroundImage = pool[i % pool.length];
      const stop = 180 + i * (calm ? 50 : 32) + Math.random() * 220;
      const timer = window.setInterval(() => (c.style.backgroundImage = pool[Math.floor(Math.random() * pool.length)]), 70);
      window.setTimeout(() => {
        window.clearInterval(timer);
        c.classList.remove("scr");
        c.style.backgroundImage = "";
      }, stop);
    });
  };
  const flicker = (el: HTMLElement) => {
    const red = getComputedStyle(document.documentElement).getPropertyValue("--red").trim();
    const chars = [...el.querySelectorAll<HTMLElement>(".ch")].filter((c) => GLYPHS[c.dataset.c ?? ""]);
    const once = () => {
      const c = chars[Math.floor(Math.random() * chars.length)];
      if (c && !c.classList.contains("scr")) {
        c.classList.add("scr");
        c.style.backgroundImage = svg(GLYPHS[c.dataset.c!], red);
        window.setTimeout(() => {
          c.classList.remove("scr");
          c.style.backgroundImage = "";
        }, 900);
      }
      window.setTimeout(once, 1600 + Math.random() * 1800);
    };
    window.setTimeout(once, 2400);
  };

  const seen = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        seen.unobserve(e.target);
        const el = e.target as HTMLElement;
        decode(el);
        if (el.dataset.pixelType === "live") flicker(el);
      }
    },
    { threshold: 0.4 },
  );
  document.querySelectorAll<HTMLElement>("[data-pixel-type]").forEach((el) => seen.observe(el));
}
