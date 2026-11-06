/**
 * The mixnet on a globe you can spin. Clients around the world send small red packets to the gateway in
 * Mumbai; they hop on raised arcs to the mix in Frankfurt, wait for its shuffled batch, and reach the exit
 * in North Virginia, which settles them (amber). Replies return, pale, through the relay in Singapore.
 * Dim packets between the nodes are cover traffic. Land is drawn as a halftone of dots.
 */
import { geoDistance, geoEquirectangular, geoInterpolate, geoOrthographic, geoPath } from "d3-geo";
import type { GeoPermissibleObjects } from "d3-geo";
import { feature } from "topojson-client";
import type { Topology } from "topojson-specification";
import landTopo from "world-atlas/land-110m.json";

type LonLat = [number, number];
const NODES: Record<string, LonLat> = {
  gateway: [72.88, 19.08],
  mix: [8.68, 50.11],
  exit: [-77.46, 38.95],
  relay: [103.82, 1.35],
};
const CLIENTS: LonLat[] = [
  [-122.4, 37.8], [-118.2, 34.1], [-74.0, 40.7], [-87.6, 41.9], [-79.4, 43.7], [-99.1, 19.4], [-46.6, -23.5],
  [-58.4, -34.6], [-70.6, -33.4], [-77.0, -12.0], [-0.1, 51.5], [2.35, 48.85], [13.4, 52.5], [-3.7, 40.4],
  [12.5, 41.9], [18.1, 59.3], [21.0, 52.2], [30.5, 50.4], [28.9, 41.0], [37.6, 55.8], [31.2, 30.0],
  [3.4, 6.5], [36.8, -1.3], [28.0, -26.2], [18.4, -33.9], [55.3, 25.2], [46.7, 24.7], [77.2, 28.6],
  [77.6, 12.97], [88.4, 22.6], [90.4, 23.8], [100.5, 13.75], [106.8, -6.2], [114.2, 22.3], [121.5, 31.2],
  [116.4, 39.9], [127.0, 37.6], [139.7, 35.7], [121.0, 14.6], [151.2, -33.9], [144.96, -37.8], [174.8, -36.8],
];
const FORWARD = ["gateway", "mix", "exit"];
const REPLY = ["exit", "relay", "mix", "gateway"];

interface Packet {
  pts: LonLat[];
  leg: number;
  t: number;
  dur: number;
  color: string;
  size: number;
  kind: "pay" | "cover" | "reply";
  waiting: boolean;
  interp: (t: number) => LonLat;
  lift: number;
}

const css = (n: string) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();

/** Land as dots: rasterize the land once to a small equirectangular canvas and keep the covered cells. */
function landDots(land: GeoPermissibleObjects): LonLat[] {
  const W = 720;
  const H = 360;
  const off = document.createElement("canvas");
  off.width = W;
  off.height = H;
  const c = off.getContext("2d", { willReadFrequently: true })!;
  const proj = geoEquirectangular().scale(W / (2 * Math.PI)).translate([W / 2, H / 2]);
  c.beginPath();
  geoPath(proj, c)(land);
  c.fill();
  const data = c.getImageData(0, 0, W, H).data;
  const dots: LonLat[] = [];
  const STEP = 1.6;
  for (let lat = -58; lat <= 80; lat += STEP) {
    const lonStep = STEP / Math.max(0.25, Math.cos((lat * Math.PI) / 180));
    for (let lon = -180; lon < 180; lon += lonStep) {
      const x = Math.floor(((lon + 180) / 360) * W);
      const y = Math.floor(((90 - lat) / 180) * H);
      if (data[(y * W + x) * 4 + 3] > 128) dots.push([lon, lat]);
    }
  }
  return dots;
}

export function mountMixnet(root: HTMLElement): void {
  const canvas = root.querySelector<HTMLCanvasElement>("canvas")!;
  const leaders = root.querySelector<SVGSVGElement>("[data-leaders]")!;
  const ctx = canvas.getContext("2d")!;
  const land = feature(landTopo as unknown as Topology, (landTopo as any).objects.land) as GeoPermissibleObjects;
  const dots = landDots(land);
  const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const pace = calm ? 0.45 : 1;
  const RED = css("--red");
  const AMBER = css("--amber");
  const MOON = css("--moon");
  const counter = root.querySelector<HTMLElement>("[data-mixed]");

  const projection = geoOrthographic().clipAngle(90).precision(0.5);
  let rotation: [number, number] = [-40, -24];
  let w = 0;
  let h = 0;
  let dpr = 1;
  const resize = () => {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    const r = canvas.getBoundingClientRect();
    w = r.width;
    h = r.height;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    projection.scale(Math.min(w, h) * 0.46).translate([w / 2, h / 2]);
  };
  new ResizeObserver(resize).observe(canvas);
  resize();

  const centre = (): LonLat => [-rotation[0], -rotation[1]];
  const visible = (p: LonLat) => geoDistance(p, centre()) < Math.PI / 2 - 0.03;
  /** A point raised above the surface: pushed out from the globe's centre on screen. */
  const lifted = (p: LonLat, lift: number): [number, number] => {
    const [x, y] = projection(p)!;
    const [cx, cy] = projection.translate();
    return [cx + (x - cx) * (1 + lift), cy + (y - cy) * (1 + lift)];
  };

  const packets: Packet[] = [];
  const pulses: { at: LonLat; color: string; born: number }[] = [];
  const startLeg = (pk: Packet) => {
    const a = pk.pts[pk.leg];
    const b = pk.pts[pk.leg + 1];
    const d = geoDistance(a, b);
    pk.interp = geoInterpolate(a, b);
    pk.lift = 0.02 + d * 0.05;
    pk.dur = (500 + d * 1400) / pace;
    pk.t = 0;
  };
  const launch = (kind: Packet["kind"], pts: LonLat[]) => {
    const pk: Packet = {
      pts,
      leg: 0,
      t: 0,
      dur: 1,
      color: kind === "pay" ? RED : kind === "reply" ? MOON : "rgba(236,234,227,0.45)",
      size: kind === "cover" ? 2 : 3,
      kind,
      waiting: false,
      interp: () => pts[0],
      lift: 0,
    };
    startLeg(pk);
    packets.push(pk);
  };
  const client = () => CLIENTS[Math.floor(Math.random() * CLIENTS.length)];
  const pay = () => launch("pay", [client(), ...FORWARD.map((n) => NODES[n])]);
  const cover = () => {
    const names = Object.keys(NODES);
    const a = names[Math.floor(Math.random() * names.length)];
    let b = a;
    while (b === a) b = names[Math.floor(Math.random() * names.length)];
    launch("cover", [NODES[a], NODES[b]]);
  };
  let mixed = 0;
  const releaseBatch = () => {
    const held = packets.filter((p) => p.waiting);
    // Shuffled, and staggered by starting each a little behind the last (t below zero waits at the mix).
    held.sort(() => Math.random() - 0.5).forEach((p, i) => {
      p.waiting = false;
      p.leg += 1;
      startLeg(p);
      p.t = (-i * 40) / p.dur;
    });
    mixed += held.length;
    if (counter) counter.textContent = mixed.toLocaleString("en-US");
    if (held.length) pulses.push({ at: NODES.mix, color: RED, born: performance.now() });
  };

  // Drag to spin, with inertia that settles back into the slow turn.
  let dragging = false;
  let last: [number, number] | null = null;
  let velocity = 0;
  canvas.addEventListener("pointerdown", (e) => {
    dragging = true;
    last = [e.clientX, e.clientY];
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!dragging || !last) return;
    const k = 90 / projection.scale();
    const dx = (e.clientX - last[0]) * k;
    rotation = [rotation[0] + dx, Math.max(-60, Math.min(60, rotation[1] - (e.clientY - last[1]) * k))];
    velocity = dx;
    last = [e.clientX, e.clientY];
  });
  const stop = () => {
    dragging = false;
    last = null;
  };
  canvas.addEventListener("pointerup", stop);
  canvas.addEventListener("pointercancel", stop);

  const lines = [...leaders.querySelectorAll<SVGPathElement>("path[data-node]")];
  const boxes = [...root.querySelectorAll<HTMLElement>("[data-callout]")];
  const routes: LonLat[][] = [
    [NODES.gateway, NODES.mix],
    [NODES.mix, NODES.exit],
    [NODES.exit, NODES.relay],
    [NODES.relay, NODES.mix],
    [NODES.mix, NODES.gateway],
  ];

  const draw = (now: number) => {
    projection.rotate(rotation);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    const [cx, cy] = projection.translate();
    const r = projection.scale();

    // the sphere: a soft warm shading and a rim
    const shade = ctx.createRadialGradient(cx - r * 0.35, cy - r * 0.4, r * 0.05, cx, cy, r * 1.02);
    shade.addColorStop(0, "rgba(236,234,227,0.07)");
    shade.addColorStop(1, "rgba(236,234,227,0.01)");
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
    ctx.fillStyle = shade;
    ctx.fill();
    ctx.strokeStyle = "rgba(236,234,227,0.22)";
    ctx.lineWidth = 1;
    ctx.stroke();

    // land, as dots that fade towards the rim
    const c0 = centre();
    for (const d of dots) {
      const dist = geoDistance(d, c0);
      if (dist > Math.PI / 2) continue;
      const [x, y] = projection(d)!;
      const a = 0.12 + 0.5 * Math.cos(dist);
      ctx.fillStyle = `rgba(236,234,227,${a.toFixed(3)})`;
      ctx.fillRect(x - 0.9, y - 0.9, 1.8, 1.8);
    }

    // client cities
    ctx.fillStyle = "rgba(236,234,227,0.75)";
    for (const c of CLIENTS) {
      if (!visible(c)) continue;
      const [x, y] = projection(c)!;
      ctx.fillRect(x - 1.5, y - 1.5, 3, 3);
    }

    // the route arcs between nodes
    ctx.lineWidth = 1;
    for (const [a, b] of routes) {
      const f = geoInterpolate(a, b);
      const lift = 0.02 + geoDistance(a, b) * 0.05;
      ctx.beginPath();
      let pen = false;
      for (let i = 0; i <= 40; i++) {
        const t = i / 40;
        const p = f(t);
        if (!visible(p)) {
          pen = false;
          continue;
        }
        const [x, y] = lifted(p, lift * Math.sin(Math.PI * t));
        if (pen) ctx.lineTo(x, y);
        else ctx.moveTo(x, y);
        pen = true;
      }
      ctx.strokeStyle = "rgba(255,74,36,0.22)";
      ctx.stroke();
    }

    // pulses at nodes
    for (let i = pulses.length - 1; i >= 0; i--) {
      const p = pulses[i];
      const age = (now - p.born) / 1200;
      if (age > 1) {
        pulses.splice(i, 1);
        continue;
      }
      if (age < 0 || !visible(p.at)) continue;
      const [x, y] = projection(p.at)!;
      ctx.beginPath();
      ctx.arc(x, y, 5 + age * 30, 0, Math.PI * 2);
      ctx.strokeStyle = p.color;
      ctx.globalAlpha = 1 - age;
      ctx.lineWidth = 1.5;
      ctx.stroke();
      ctx.globalAlpha = 1;
    }

    // packets, each with a short trail
    for (const pk of packets) {
      for (let k = 5; k >= 0; k--) {
        const t = Math.max(0, pk.t - k * 0.022);
        const p = pk.interp(t);
        if (!visible(p)) continue;
        const [x, y] = lifted(p, pk.lift * Math.sin(Math.PI * t));
        const s = pk.size * (k === 0 ? 1 : 0.8);
        ctx.globalAlpha = k === 0 ? 1 : 0.4 - k * 0.06;
        ctx.fillStyle = pk.color;
        ctx.fillRect(x - s / 2, y - s / 2, s, s);
      }
      ctx.globalAlpha = 1;
    }

    // the four nodes
    for (const [name, at] of Object.entries(NODES)) {
      if (!visible(at)) continue;
      const [x, y] = projection(at)!;
      ctx.beginPath();
      ctx.arc(x, y, 5.5, 0, Math.PI * 2);
      ctx.fillStyle = name === "exit" ? AMBER : name === "relay" ? MOON : RED;
      ctx.fill();
      ctx.lineWidth = 2.5;
      ctx.strokeStyle = "#0e0d0c";
      ctx.stroke();
    }

    // leader lines from each callout to its node
    const box = root.getBoundingClientRect();
    const cr = canvas.getBoundingClientRect();
    lines.forEach((line, i) => {
      const name = line.dataset.node!;
      const b = boxes[i].getBoundingClientRect();
      if (!b.width) return;
      const show = visible(NODES[name]);
      const [nx, ny] = projection(NODES[name])!;
      const tx = cr.left - box.left + nx;
      const ty = cr.top - box.top + ny;
      const left = b.left + b.width / 2 < box.left + box.width / 2;
      const sx = (left ? b.right : b.left) - box.left;
      const sy = b.top + b.height / 2 - box.top;
      const elbow = sx + (left ? 1 : -1) * 48;
      line.setAttribute("d", `M${sx} ${sy} H${elbow} L${tx} ${ty}`);
      line.style.opacity = show ? "1" : "0";
      boxes[i].classList.toggle("is-far", !show);
    });
  };

  let running = true;
  new IntersectionObserver(([e]) => (running = e.isIntersecting)).observe(root);
  let prev = performance.now();
  let payDebt = 0;
  let coverDebt = 0;
  let sinceBatch = 0;
  const simulate = (dt: number, now: number) => {
    if (!dragging) {
      velocity *= 0.95;
      rotation = [rotation[0] + velocity + dt * 0.006 * pace, rotation[1] + (-24 - rotation[1]) * 0.01];
    }
    payDebt += dt * 0.012 * pace;
    coverDebt += dt * 0.006 * pace;
    for (; payDebt >= 1; payDebt--) pay();
    for (; coverDebt >= 1; coverDebt--) cover();
    sinceBatch += dt;
    if (sinceBatch > 1500 / pace) {
      sinceBatch = 0;
      releaseBatch();
    }
    for (let i = packets.length - 1; i >= 0; i--) {
      const pk = packets[i];
      if (pk.waiting) continue;
      pk.t += dt / pk.dur;
      if (pk.t < 1) continue;
      const arrived = pk.pts[pk.leg + 1];
      if (pk.leg + 1 >= pk.pts.length - 1) {
        packets.splice(i, 1);
        if (pk.kind === "pay") {
          pulses.push({ at: arrived, color: AMBER, born: now });
          launch("reply", [...REPLY.map((n) => NODES[n]), client()]);
        }
        continue;
      }
      if (pk.kind === "pay" && arrived === NODES.mix) {
        pk.waiting = true;
        pk.t = 1;
        continue;
      }
      pk.leg += 1;
      startLeg(pk);
    }
  };
  const frame = (now: number) => {
    const dt = Math.min(now - prev, 50);
    prev = now;
    if (running) {
      simulate(dt, now);
      draw(now);
    }
    requestAnimationFrame(frame);
  };
  // Run the network for a few seconds first, so the globe opens with traffic on it.
  for (let i = 0; i < 500; i++) simulate(20, prev);
  requestAnimationFrame(frame);
}
