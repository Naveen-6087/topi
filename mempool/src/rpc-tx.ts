// Rebuilds a frame transaction from ethrex's eth_getTransactionByHash JSON, so a mined settle can be
// replayed against the mempool (the node has no eth_getRawTransactionByHash).
import { fromHex, type Frame, type FrameSignature, type FrameTransaction, type Hex } from "../../sdk/src/index.ts";

interface RpcFrame {
  mode: Hex;
  flags: Hex;
  to: Hex | null;
  gasLimit: Hex;
  stateGasLimit: Hex;
  value: Hex;
  data: Hex;
}

interface RpcSignature {
  scheme: Hex;
  signer: Hex | null;
  msg: Hex;
  signature: Hex;
}

export interface RpcFrameTransaction {
  type: Hex;
  hash: Hex;
  chainId: Hex;
  nonceKeys: Hex[];
  nonceSeq: Hex;
  sender: Hex;
  frames: RpcFrame[];
  signatures: RpcSignature[];
  maxPriorityFeePerGas: Hex;
  maxFeePerGas: Hex;
  maxFeePerBlobGas: Hex;
  blobVersionedHashes: Hex[];
}

const big = (v: Hex | null | undefined) => (v == null || v === "0x" ? 0n : BigInt(v));

export function fromRpcTransaction(t: RpcFrameTransaction): FrameTransaction {
  if (big(t.type) !== 6n) throw new Error(`transaction ${t.hash} is type ${t.type}, not a frame transaction`);
  const frames: Frame[] = t.frames.map((f) => ({
    mode: big(f.mode),
    flags: big(f.flags),
    target: f.to == null ? null : BigInt(f.to),
    gasLimit: big(f.gasLimit),
    stateLimit: big(f.stateGasLimit),
    value: big(f.value),
    data: fromHex(f.data),
  }));
  const signatures: FrameSignature[] = t.signatures.map((s) => ({
    scheme: big(s.scheme),
    signer: big(s.signer),
    msg: fromHex(s.msg),
    signature: fromHex(s.signature),
  }));
  return {
    chainId: big(t.chainId),
    nonceKeys: t.nonceKeys.map(big),
    nonceSeq: big(t.nonceSeq),
    sender: BigInt(t.sender),
    frames,
    signatures,
    maxPriorityFee: big(t.maxPriorityFeePerGas),
    maxFee: big(t.maxFeePerGas),
    maxBlobFee: big(t.maxFeePerBlobGas),
    blobHashes: t.blobVersionedHashes.map((h) => fromHex(h)),
  };
}
