/**
 * A sparse field of packets drifting behind the hero. Most are faint; a few are payments (red) or proofs
 * (amber). Packets near the pointer light up and lean away from it.
 */
interface Mote {
  x: number;
  y: number;
  v: number;
  s: number;
  a: number;
  c: string;
  dy: number;
}

export function mountDrift(canvas: HTMLCanvasElement): void {
  const ctx = canvas.getContext("2d")!;
  const pace = matchMedia("(prefers-reduced-motion: reduce)").matches ? 0.4 : 1;
  let w = 0;
  let h = 0;
  let dpr = 1;
  const motes: Mote[] = [];
  const make = (x?: number): Mote => {
    const r = Math.random();
    return {
      x: x ?? Math.random() * w,
      y: Math.random() * h,
      v: (8 + Math.random() * 26) * pace,
      s: r < 0.1 ? 3 : 2,
      a: 0.12 + Math.random() * 0.22,
      c: r < 0.08 ? "255,74,36" : r < 0.13 ? "245,184,61" : "236,234,227",
      dy: 0,
    };
  };
  const resize = () => {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    const r = canvas.getBoundingClientRect();
    w = r.width;
    h = r.height;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const want = Math.round((w * h) / 9000);
    while (motes.length < want) motes.push(make());
    motes.length = Math.min(motes.length, want);
  };
  new ResizeObserver(resize).observe(canvas);
  resize();

  let pointer: { x: number; y: number } | null = null;
  window.addEventListener("pointermove", (e) => {
    const r = canvas.getBoundingClientRect();
    pointer = { x: e.clientX - r.left, y: e.clientY - r.top };
  });

  let running = true;
  new IntersectionObserver(([e]) => (running = e.isIntersecting)).observe(canvas);
  let prev = performance.now();
  const frame = (now: number) => {
    const dt = Math.min(now - prev, 50) / 1000;
    prev = now;
    if (running) {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      for (const m of motes) {
        m.x += m.v * dt;
        if (m.x > w + 10) Object.assign(m, make(-10));
        let glow = 0;
        if (pointer) {
          const dx = m.x - pointer.x;
          const dy = m.y - pointer.y;
          const d = Math.hypot(dx, dy);
          if (d < 140) {
            glow = 1 - d / 140;
            m.dy += (dy >= 0 ? 1 : -1) * glow * 30 * dt;
          }
        }
        m.dy *= 0.96;
        m.y += m.dy;
        const a = Math.min(1, m.a + glow * 0.8);
        ctx.fillStyle = `rgba(${m.c},${a * 0.25})`;
        ctx.fillRect(m.x - m.v * 0.5, m.y, m.v * 0.5, 1);
        ctx.fillStyle = `rgba(${m.c},${a})`;
        ctx.fillRect(m.x, m.y - m.s / 2, m.s, m.s);
      }
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}
