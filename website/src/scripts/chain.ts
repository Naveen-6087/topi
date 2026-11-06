/** Live figures from the chain 8141 testnet, read straight from its public RPC. */

const RPC = "https://rpc1.privacy.ethrex.xyz";
const POOL = "0x66268c9e7735f331c48c30eecf2fe2e76b80a9d7";
const SETTLED = "0xd6bc57af9bdeff672c39eb7ac8725e83883c78f01ea44f6ce017a99e3ef5bb3c";
const POOL_DEPLOYED = 544_500;

async function rpc<T>(method: string, params: unknown[]): Promise<T> {
  const res = await fetch(RPC, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  const body = (await res.json()) as { result?: T; error?: unknown };
  if (body.error !== undefined || body.result === undefined) throw new Error(method);
  return body.result;
}

const number = (n: bigint | number) => Number(n).toLocaleString("en-US");
const ether = (wei: bigint) => `${(Number(wei) / 1e18).toFixed(3)} ETH`;

/** Counts a figure up from what the page shows to the new value, so a fresh block visibly ticks over. */
function countTo(el: HTMLElement, target: number, format: (n: number) => string): void {
  const from = Number(el.dataset.value ?? el.textContent?.replace(/[^\d.]/g, "") ?? 0) || 0;
  el.dataset.value = String(target);
  if (from === target || matchMedia("(prefers-reduced-motion: reduce)").matches) {
    el.textContent = format(target);
    return;
  }
  const start = performance.now();
  const step = (now: number) => {
    const k = Math.min((now - start) / 900, 1);
    const eased = 1 - Math.pow(1 - k, 3);
    el.textContent = format(from + (target - from) * eased);
    if (k < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

export function watchChain(): void {
  const set = (sel: string, text: string) => {
    const value = Number(text.replace(/[^\d.]/g, ""));
    const isEther = text.endsWith("ETH");
    for (const el of document.querySelectorAll<HTMLElement>(sel)) {
      countTo(el, value, (n) => (isEther ? `${n.toFixed(3)} ETH` : Math.round(n).toLocaleString("en-US")));
    }
  };
  const refresh = async () => {
    try {
      const [block, balance, logs] = await Promise.all([
        rpc<string>("eth_blockNumber", []),
        rpc<string>("eth_getBalance", [POOL, "latest"]),
        rpc<{ blockNumber: string }[]>("eth_getLogs", [{ address: POOL, topics: [SETTLED], fromBlock: `0x${POOL_DEPLOYED.toString(16)}`, toBlock: "latest" }]),
      ]);
      set("[data-block]", number(BigInt(block)));
      set("[data-pool]", ether(BigInt(balance)));
      const lastSettle = logs.at(-1);
      if (lastSettle) set("[data-settle]", number(BigInt(lastSettle.blockNumber)));
    } catch {
      // the figures already in the page are the last known ones
    }
  };
  refresh();
  window.setInterval(refresh, 12_000);
}
