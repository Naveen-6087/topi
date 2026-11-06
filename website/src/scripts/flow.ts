/**
 * The mixnet as a box of three hops. Packets stream in from the left, one colour per sender, so each can
 * be traced. Every hop holds them, adds cover traffic, and passes a shuffled batch on; with each hop they
 * lose more of their colour. What leaves on the right is identical and in a new order. The pointer pushes
 * packets in flight aside.
 */

const SENDERS = ["#ff4a24", "#ff8a5c", "#f5b83d", "#eceae3", "#ff6f91", "#7dd3b0", "#c9a7ff", "#ffd58a"];
const NOTE = [236, 234, 227];
const OUT = [245, 184, 61];
const HOPS = 3;

type Phase = "in" | "hop" | "out";
interface Packet {
  x: number;
  y: number;
  vx: number;
  lane: number;
  y0: number;
  base: number[];
  color: number[];
  phase: Phase;
  hop: number;
  age: number;
  tx: number;
  ty: number;
  push: number;
}

const hex = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
const rgb = (c: number[], a = 1) => `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a})`;
const mix = (a: number[], b: number[], k: number) => a.map((v, i) => v + (b[i] - v) * k);
const smooth = (k: number) => k * k * (3 - 2 * k);

export function mountFlow(root: HTMLElement): void {
  const canvas = root.querySelector<HTMLCanvasElement>("canvas")!;
  const ctx = canvas.getContext("2d")!;
  const pace = matchMedia("(prefers-reduced-motion: reduce)").matches ? 0.45 : 1;
  const senders = SENDERS.map(hex);
  const LANES = SENDERS.length;
  const mono = getComputedStyle(document.documentElement).getPropertyValue("--code").trim();

  let w = 0;
  let h = 0;
  let dpr = 1;
  let box = { x: 0, y: 0, w: 0, h: 0 };
  const resize = () => {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    const r = canvas.getBoundingClientRect();
    w = r.width;
    h = r.height;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const bw = Math.max(210, Math.min(440, w * 0.36));
    const bh = Math.max(130, Math.min(210, h * 0.56));
    box = { x: (w - bw) / 2, y: (h - bh) / 2, w: bw, h: bh };
    root.style.setProperty("--box-top", `${box.y}px`);
  };
  new ResizeObserver(resize).observe(canvas);
  resize();

  const cell = (hop: number) => {
    const cw = box.w / HOPS;
    return { x: box.x + hop * cw + 10 + Math.random() * (cw - 20), y: box.y + 22 + Math.random() * (box.h - 32) };
  };
  const laneY = (lane: number) => h * 0.1 + ((lane + 0.5) / LANES) * h * 0.8;
  const mouth = (lane: number) => box.y + 18 + ((lane + 0.5) / LANES) * (box.h - 28);

  const packets: Packet[] = [];
  let debt = 0;
  let sinceBatch = 0;

  const spawn = () => {
    const lane = Math.floor(Math.random() * LANES);
    const y0 = laneY(lane) + (Math.random() - 0.5) * 10;
    const c = senders[lane];
    packets.push({ x: -6, y: y0, vx: (120 + Math.random() * 90) * pace, lane, y0, base: c, color: c, phase: "in", age: 0, tx: 0, ty: 0, push: 0 });
  };
  const enter = (p: Packet, hop: number) => {
    p.phase = "hop";
    p.hop = hop;
    p.age = 0;
    const c = cell(hop);
    p.tx = c.x;
    p.ty = c.y;
  };
  // Cover traffic: dim packets that appear inside a hop and leave indistinguishable from the rest.
  const cover = () => {
    const hop = Math.floor(Math.random() * HOPS);
    const c = cell(hop);
    const p: Packet = { x: c.x, y: c.y, vx: 0, lane: 0, y0: 0, base: NOTE, color: [120, 117, 110], phase: "hop", hop, age: 0, tx: c.x, ty: c.y, push: 0 };
    packets.push(p);
  };
  const batch = () => {
    for (let hop = HOPS - 1; hop >= 0; hop--) {
      const here = packets.filter((p) => p.phase === "hop" && p.hop === hop && p.age > 0.45);
      const k = Math.min(here.length, 3 + Math.floor(Math.random() * 6) + Math.max(0, here.length - 40));
      for (let i = 0; i < k; i++) {
        const j = i + Math.floor(Math.random() * (here.length - i));
        [here[i], here[j]] = [here[j], here[i]];
        const p = here[i];
        if (hop < HOPS - 1) enter(p, hop + 1);
        else {
          p.phase = "out";
          p.age = -i * 0.05;
          p.lane = Math.floor(Math.random() * LANES);
          p.y0 = laneY(p.lane) + (Math.random() - 0.5) * 10;
          p.vx = (140 + Math.random() * 90) * pace;
        }
      }
    }
  };

  let pointer: { x: number; y: number } | null = null;
  canvas.addEventListener("pointermove", (e) => {
    const r = canvas.getBoundingClientRect();
    pointer = { x: e.clientX - r.left, y: e.clientY - r.top };
  });
  canvas.addEventListener("pointerleave", () => (pointer = null));

  const drawBox = (t: number) => {
    const { x, y, w: bw, h: bh } = box;
    const cw = bw / HOPS;
    ctx.fillStyle = "rgba(236,234,227,0.06)";
    for (let gx = x + 8; gx < x + bw - 4; gx += 12) for (let gy = y + 8; gy < y + bh - 4; gy += 12) ctx.fillRect(gx, gy, 2, 2);
    // the outer wall drifts, so the box reads as running
    ctx.fillStyle = "rgba(236,234,227,0.35)";
    const step = 8;
    const shift = ((t * 0.012) % step) | 0;
    for (let i = shift; i < bw; i += step) {
      ctx.fillRect(x + i, y, 3, 3);
      ctx.fillRect(x + bw - i, y + bh - 3, 3, 3);
    }
    for (let i = shift; i < bh; i += step) {
      ctx.fillRect(x + bw - 3, y + i, 3, 3);
      ctx.fillRect(x, y + bh - i, 3, 3);
    }
    // walls between the hops
    ctx.fillStyle = "rgba(236,234,227,0.16)";
    for (let k = 1; k < HOPS; k++) for (let i = 6; i < bh - 6; i += 8) ctx.fillRect(x + k * cw - 1, y + i, 2, 3);
    ctx.font = `10px ${mono}`;
    ctx.fillStyle = "rgba(236,234,227,0.4)";
    for (let k = 0; k < HOPS; k++) ctx.fillText(`hop ${k + 1}`, x + k * cw + 10, y + 16);
    // bracket corners
    ctx.fillStyle = "#eceae3";
    const L = 18;
    const B = 3;
    for (const [cx, cy, sx, sy] of [
      [x - 6, y - 6, 1, 1],
      [x + bw + 6, y - 6, -1, 1],
      [x - 6, y + bh + 6, 1, -1],
      [x + bw + 6, y + bh + 6, -1, -1],
    ]) {
      ctx.fillRect(sx > 0 ? cx : cx - L, sy > 0 ? cy : cy - B, L, B);
      ctx.fillRect(sx > 0 ? cx : cx - B, sy > 0 ? cy : cy - L, B, L);
    }
  };

  const tick = (dt: number) => {
    const s = dt / 1000;
    for (let i = packets.length - 1; i >= 0; i--) {
      const p = packets[i];
      p.age += s;
      if (p.phase === "in") {
        p.x += p.vx * s;
        const k = smooth(Math.min(Math.max(p.x / box.x, 0), 1));
        p.y = p.y0 + (mouth(p.lane) - p.y0) * k + Math.sin(p.age * 3 + p.lane) * 2 * (1 - k) + p.push;
        if (p.x >= box.x + 4) enter(p, 0);
      } else if (p.phase === "hop") {
        p.x += (p.tx - p.x) * Math.min(1, s * 5);
        p.y += (p.ty - p.y) * Math.min(1, s * 5);
        // each hop takes away more of the sender's colour
        p.color = mix(p.color, mix(p.base, NOTE, (p.hop + 1) / HOPS), Math.min(1, s * 2.5));
        if (Math.random() < s * 0.9) {
          const c = cell(p.hop);
          p.tx = c.x;
          p.ty = c.y;
        }
      } else {
        if (p.age < 0) continue;
        p.x += p.vx * s;
        const k = smooth(Math.min(Math.max((p.x - box.x - box.w) / (w - box.x - box.w), 0), 1));
        p.y = mouth(p.lane) + (p.y0 - mouth(p.lane)) * k + p.push;
        p.color = mix(p.color, OUT, Math.min(1, s * 4));
        if (p.x > w + 8) packets.splice(i, 1);
      }
      if (pointer && p.phase !== "hop") {
        const dx = p.x - pointer.x;
        const dy = p.y - pointer.y;
        const d2 = dx * dx + dy * dy;
        if (d2 < 5000) p.push += (dy >= 0 ? 1 : -1) * (1 - d2 / 5000) * 90 * s;
      }
      p.push *= 1 - Math.min(1, s * 1.5);
    }
  };

  const draw = (t: number) => {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    drawBox(t);
    for (const p of packets) {
      if (p.phase === "out" && p.age < 0) continue;
      const size = p.phase === "hop" ? 3.5 : 4;
      if (p.phase !== "hop") {
        ctx.fillStyle = rgb(p.color, 0.18);
        ctx.fillRect(p.x - p.vx * 0.12, p.y - 0.5, p.vx * 0.12, 1.5);
      }
      ctx.fillStyle = rgb(p.color, p.phase === "hop" ? 0.8 : 1);
      ctx.fillRect(p.x - size / 2, p.y - size / 2, size, size);
    }
  };

  const step = (dt: number) => {
    debt += dt * 0.1 * pace;
    for (; debt >= 1; debt--) spawn();
    if (Math.random() < dt * 0.004 * pace) cover();
    sinceBatch += dt;
    if (sinceBatch > 650 / pace) {
      sinceBatch = 0;
      batch();
    }
    tick(dt);
  };

  let running = true;
  new IntersectionObserver(([e]) => (running = e.isIntersecting)).observe(root);
  let prev = performance.now();
  const frame = (now: number) => {
    const dt = Math.min(now - prev, 50);
    prev = now;
    if (running) {
      step(dt);
      draw(now);
    }
    requestAnimationFrame(frame);
  };
  // Run for a few seconds before the first frame, so the box opens already full.
  for (let i = 0; i < 400; i++) step(20);
  requestAnimationFrame(frame);
}
