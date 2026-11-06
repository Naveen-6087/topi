// Deterministic frame transactions used by the encoding tests. The builder is library-agnostic
// (it receives the signing functions), so the same cases produced the recorded vectors.

export interface Lib<Tx> {
  signTransaction(tx: Tx, key: Uint8Array, ...after: any[]): Tx;
  sigHash(tx: Tx): Uint8Array;
}

/** A fixed test key (not used anywhere on chain). */
export const TEST_KEY = new Uint8Array(32).map((_, i) => i + 1);

const bytes = (n: number, seed: number) => new Uint8Array(n).map((_, i) => (i * 31 + seed) & 0xff);
const arbitrary = () => ({ scheme: 0n, signer: 0n, msg: new Uint8Array(0), signature: new Uint8Array(0) });

const POOL = 0x66268c9e7735f331c48c30eecf2fe2e76b80a9d7n;
const PAYMASTER = 0x4df9fe3b7054685f6faf7c5deb5392d527035c62n;
const ACCOUNT = 0xb6f6dc5f1139568fb72e34de4174c9496a6c7872n;

function base(): any {
  return {
    chainId: 8141n,
    nonceKeys: [0x1234567890abcdef1234567890abcdefn],
    nonceSeq: 0n,
    sender: POOL,
    frames: [],
    signatures: [],
    maxPriorityFee: 1_000_000_000n,
    maxFee: 3_000_000_014n,
    maxBlobFee: 0n,
    blobHashes: [],
  };
}

/** Each case returns a fully built (and, where applicable, signed) transaction. */
export function cases<Tx>(lib: Lib<Tx>): Record<string, () => Tx> {
  return {
    "unsigned single frame": () => {
      const tx = base();
      tx.frames = [{ mode: 2n, flags: 0n, target: 0xdeadn, gasLimit: 50_000n, stateLimit: 0n, value: 1n, data: new Uint8Array(0) }];
      return tx;
    },
    "ECDSA-signed settle shape": () => {
      const tx = base();
      tx.frames = [
        { mode: 1n, flags: 2n, target: POOL, gasLimit: 10_000n, stateLimit: 0n, value: 0n, data: new Uint8Array(4) },
        { mode: 1n, flags: 1n, target: PAYMASTER, gasLimit: 60_000n, stateLimit: 110_000n, value: 0n, data: new Uint8Array(4) },
        { mode: 2n, flags: 0n, target: POOL, gasLimit: 14_600_000n, stateLimit: 5_000_000n, value: 0n, data: bytes(2_000, 7) },
      ];
      return lib.signTransaction(tx, TEST_KEY);
    },
    "ARBITRARY witness (PQ paymaster)": () => {
      const tx = base();
      tx.frames = [
        { mode: 1n, flags: 2n, target: POOL, gasLimit: 10_000n, stateLimit: 0n, value: 0n, data: new Uint8Array(4) },
        { mode: 1n, flags: 1n, target: PAYMASTER, gasLimit: 150_000n, stateLimit: 110_000n, value: 0n, data: new Uint8Array(4) },
        { mode: 2n, flags: 0n, target: POOL, gasLimit: 14_600_000n, stateLimit: 5_000_000n, value: 0n, data: bytes(70_000, 3) },
      ];
      tx.signatures = [arbitrary()];
      // a stand-in witness: the hash it would sign is recorded separately
      tx.signatures[0].signature = bytes(5_684, 11);
      return tx;
    },
    "ARBITRARY witness (PQ account)": () => {
      const tx = base();
      tx.sender = ACCOUNT;
      tx.frames = [
        { mode: 1n, flags: 3n, target: ACCOUNT, gasLimit: 120_000n, stateLimit: 110_000n, value: 0n, data: Uint8Array.of(0x82, 0xf4, 0xff, 0xda) },
        { mode: 2n, flags: 0n, target: POOL, gasLimit: 9_000_000n, stateLimit: 8_000_000n, value: 10_000_000_000_000_000n, data: bytes(260, 5) },
      ];
      tx.signatures = [arbitrary()];
      tx.signatures[0].signature = bytes(5_684, 13);
      return tx;
    },
    "ECDSA then ARBITRARY witness": () => {
      const tx = base();
      tx.sender = ACCOUNT;
      tx.frames = [
        { mode: 1n, flags: 3n, target: ACCOUNT, gasLimit: 150_000n, stateLimit: 110_000n, value: 0n, data: Uint8Array.of(0x82, 0xf4, 0xff, 0xda) },
        { mode: 2n, flags: 0n, target: 0xdeadn, gasLimit: 50_000n, stateLimit: 0n, value: 1n, data: new Uint8Array(0) },
      ];
      const signed = lib.signTransaction(tx, TEST_KEY, arbitrary()) as any;
      signed.signatures[1].signature = bytes(5_684, 17);
      return signed;
    },
    "multi-frame, targetless, two nonce keys, blobs": () => {
      const tx = base();
      tx.nonceKeys = [5n, 0xffffffffffffffffffffffffffffffffn];
      tx.nonceSeq = 300n;
      tx.maxBlobFee = 7n;
      tx.blobHashes = [bytes(32, 1), bytes(32, 2)];
      tx.frames = [
        { mode: 1n, flags: 3n, target: POOL, gasLimit: 80_000n, stateLimit: 0n, value: 0n, data: Uint8Array.of(0x7f) },
        { mode: 0n, flags: 4n, target: null, gasLimit: 30_000n, stateLimit: 0n, value: 0n, data: Uint8Array.of(0x80) },
        { mode: 2n, flags: 0n, target: PAYMASTER, gasLimit: 1n, stateLimit: 1n << 40n, value: (1n << 255n) + 1n, data: bytes(55, 9) },
        { mode: 2n, flags: 0n, target: 0n, gasLimit: 0n, stateLimit: 0n, value: 0n, data: bytes(56, 9) },
      ];
      return lib.signTransaction(tx, TEST_KEY);
    },
  };
}
