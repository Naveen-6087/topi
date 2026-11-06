// Deposits and settles, shared by wallet.ts, demo.ts, settle.ts and exit-prover.ts. Nothing here
// broadcasts unless `send` is true; without it, frame transactions are only simulated.
import { writeFileSync } from "node:fs";

import {
  EXECUTION_GAS_CAP,
  MODE,
  addressOf,
  checksumAddress,
  buildAccountTransaction,
  addArbitraryWitness,
  buildSettleTransaction,
  decodePoolLog,
  depositCalldata,
  encodeRawHex,
  executionCapUsage,
  signEip1559,
  signSettleTransaction,
  unpackDigest,
  type AccountDeployment,
  type FrameSimulation,
  type FrameTransaction,
  type OperatorSigner,
  type PoolDeployment,
  type RpcClient,
  type SettleLimits,
  type TransactionReceipt,
} from "../../sdk/src/index.ts";
import { readJson } from "./common.ts";

/** A note file from `pool-wallet keygen`, extended with its deposit once made. */
export interface NoteFile {
  precommitment: number[];
  amount?: string;
  label?: number[];
  leaf_index?: number;
  label_index?: number;
  deposit_tx?: string;
  deposit_block?: number;
  depositor?: string;
  [key: string]: unknown;
}

export const summarizeSimulation = (sim: FrameSimulation) =>
  JSON.stringify({
    valid: sim.valid,
    violation: sim.violation,
    payer: sim.payer,
    status: sim.executionStatus,
    error: sim.executionError,
    gasUsed: sim.gasUsed == null ? undefined : BigInt(sim.gasUsed).toString(),
  });

export const describeReceipt = (rc: TransactionReceipt) =>
  `mined block ${BigInt(rc.blockNumber)} status ${rc.status} gasUsed ${BigInt(rc.gasUsed)} payer ${rc.payer ?? "-"}` +
  ` frames [${(rc.frameReceipts ?? []).map((f) => f.status).join(",")}] logs ${rc.logs.length}`;

/** Records a mined deposit in the note file; returns the updated note. */
function recordDeposit(noteFile: string, note: NoteFile, receipt: TransactionReceipt, depositor: string): NoteFile {
  if (receipt.status !== "0x1") throw new Error(`deposit failed in ${receipt.transactionHash}`);
  const event = receipt.logs.map(decodePoolLog).find((e) => e?.kind === "deposited");
  if (!event || event.kind !== "deposited") throw new Error(`no Deposited event in ${receipt.transactionHash}`);
  Object.assign(note, {
    amount: event.amount.toString(),
    label: unpackDigest(event.label),
    leaf_index: Number(event.leafIndex),
    label_index: Number(event.index),
    deposit_tx: receipt.transactionHash,
    deposit_block: Number(BigInt(receipt.blockNumber)),
    depositor,
  });
  writeFileSync(noteFile, JSON.stringify(note, null, 1));
  return note;
}

export interface DepositResult {
  note?: NoteFile;
  simulation?: FrameSimulation;
  receipt?: TransactionReceipt;
}

/** Deposits a note from the leanSPHINCS PQ account (frame transaction, ARBITRARY witness). */
export async function depositFromAccount(opts: {
  rpc: RpcClient;
  deployment: PoolDeployment;
  account: AccountDeployment;
  signer: Parameters<typeof addArbitraryWitness>[1];
  noteFile: string;
  amount: bigint;
  send: boolean;
}): Promise<DepositResult> {
  const note = readJson<NoteFile>(opts.noteFile);
  if (note.deposit_tx) throw new Error(`${opts.noteFile} is already deposited (${note.deposit_tx})`);
  const tx = buildAccountTransaction({
    account: opts.account.account,
    verifySelector: opts.account.verifySelector,
    chainId: await opts.rpc.chainId(),
    fees: await opts.rpc.feeCaps(),
    call: {
      target: BigInt(opts.deployment.pool),
      value: opts.amount,
      data: depositCalldata(note.precommitment),
      gasLimit: 9_000_000n,
      stateLimit: 8_000_000n,
    },
  });
  await addArbitraryWitness(tx, opts.signer);
  const raw = encodeRawHex(tx);
  const simulation = await opts.rpc.simulateFrameTransaction(raw);
  if (!opts.send) return { simulation };
  if (!simulation.valid) throw new Error(`the deposit does not validate: ${simulation.violation}`);
  const receipt = await opts.rpc.waitForReceipt(await opts.rpc.sendRawTransaction(raw));
  return { simulation, receipt, note: recordDeposit(opts.noteFile, note, receipt, opts.account.account) };
}

/** Deposits a note from an ECDSA account (EIP-1559 transaction). */
export async function depositFromKey(opts: {
  rpc: RpcClient;
  deployment: PoolDeployment;
  key: Uint8Array;
  noteFile: string;
  amount: bigint;
  send: boolean;
  gasLimit?: bigint;
}): Promise<DepositResult & { from: string }> {
  const note = readJson<NoteFile>(opts.noteFile);
  if (note.deposit_tx) throw new Error(`${opts.noteFile} is already deposited (${note.deposit_tx})`);
  const from = checksumAddress(addressOf(opts.key));
  if (!opts.send) return { from };
  const fees = await opts.rpc.feeCaps();
  const raw = signEip1559(
    {
      chainId: await opts.rpc.chainId(),
      nonce: await opts.rpc.transactionCount(from),
      maxPriorityFee: fees.maxPriorityFee,
      maxFee: fees.maxFee,
      gasLimit: opts.gasLimit ?? 15_500_000n,
      to: BigInt(opts.deployment.pool),
      value: opts.amount,
      data: depositCalldata(note.precommitment),
    },
    opts.key,
  );
  const receipt = await opts.rpc.waitForReceipt(await opts.rpc.sendRawTransaction(raw));
  return { from, receipt, note: recordDeposit(opts.noteFile, note, receipt, from) };
}

/** Negative variants of the settle transaction, for checking the pool's and paymaster's policy. */
export const SETTLE_VARIANTS = ["valid", "no-signature", "other-target", "deposit-selector", "extra-frame", "corrupt-pq"] as const;
export type SettleVariant = (typeof SETTLE_VARIANTS)[number];

export interface SettleResult {
  tx: FrameTransaction;
  raw: string;
  simulation: FrameSimulation;
  receipt?: TransactionReceipt;
}

/** Builds, signs as the paymaster's operator, simulates and (with `send`) broadcasts a settle. */
export async function settle(opts: {
  rpc: RpcClient;
  deployment: PoolDeployment;
  signer: OperatorSigner;
  proofCall: Uint8Array;
  fields: [number[], number[]];
  limits?: Partial<SettleLimits>;
  variant?: SettleVariant;
  send: boolean;
  /** Submit through this ERC-8421 alternative mempool (eth_sendFrameTransaction) instead of straight to the node. */
  mempool?: RpcClient;
  log?: (line: string) => void;
}): Promise<SettleResult> {
  const log = opts.log ?? console.log;
  const variant = opts.variant ?? "valid";
  const tx = buildSettleTransaction({
    deployment: opts.deployment,
    chainId: await opts.rpc.chainId(),
    fees: await opts.rpc.feeCaps(),
    proofCall: opts.proofCall,
    fields: opts.fields,
    limits: opts.limits,
  });
  if (variant === "other-target") tx.frames[2].target = BigInt(opts.deployment.paymaster);
  if (variant === "deposit-selector") tx.frames[2].data.set([0x09, 0xd0, 0x09, 0xb8], 0);
  if (variant === "extra-frame") {
    tx.frames.push({ mode: MODE.DEFAULT, flags: 0n, target: BigInt(opts.deployment.pool), gasLimit: 30_000n, stateLimit: 0n, value: 0n, data: new Uint8Array(0) });
  }
  if (variant !== "no-signature") await signSettleTransaction(tx, opts.deployment, opts.signer);
  if (variant === "corrupt-pq") {
    const witness = tx.signatures[tx.signatures.length - 1].signature;
    if (witness.length < 201) throw new Error("corrupt-pq needs a PQ paymaster deployment");
    witness[200] ^= 1;
  }
  log(`declared execution against the 2^24 cap: ${executionCapUsage(tx)} / ${EXECUTION_GAS_CAP}`);
  const raw = encodeRawHex(tx);
  log(`pool spend (${variant}): ${(raw.length - 2) / 2} bytes, ${tx.frames.length} frames`);
  const simulation = await opts.rpc.simulateFrameTransaction(raw);
  log(`simulate: ${summarizeSimulation(simulation)}`);
  if (!opts.send) return { tx, raw, simulation };
  const hash = opts.mempool ? await opts.mempool.sendFrameTransaction(tx) : await opts.rpc.sendRawTransaction(raw);
  log(opts.mempool ? `admitted by the mempool ${hash}; waiting for it to settle` : `sent ${hash}`);
  // Through a mempool the settle may wait behind others from the same pool sender, so wait longer.
  const receipt = await opts.rpc.waitForReceipt(hash, { attempts: opts.mempool ? 300 : 40 });
  log(describeReceipt(receipt));
  return { tx, raw, simulation, receipt };
}
