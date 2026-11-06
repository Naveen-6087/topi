// What the mempool asks of the chain. An interface so tests can stand in for the node.
import {
  RpcClient,
  decodeWords,
  encodeCall,
  fromHex,
  keccak256,
  toHex,
  word,
  concat,
  encodeBytesTail,
  selector,
  type FrameSimulation,
  type Hex,
  type TransactionReceipt,
} from "../../sdk/src/index.ts";

export interface Chain {
  blockNumber(): Promise<bigint>;
  /** keccak256 of the account's runtime code. */
  codeHash(address: bigint): Promise<Hex>;
  spent(pool: bigint, nullifier: bigint): Promise<boolean>;
  knownPoolRoot(pool: bigint, root: bigint): Promise<boolean>;
  knownAssociationRoot(pool: bigint, root: bigint): Promise<boolean>;
  /** The shared leanSPHINCS verifier's verdict on a witness. */
  verifyLeanSphincs(verifier: bigint, pkSeed: bigint, pkRoot: bigint, message: Uint8Array, signature: Uint8Array): Promise<boolean>;
  simulate(raw: Uint8Array): Promise<FrameSimulation>;
  send(raw: Uint8Array): Promise<Hex>;
  receipt(hash: Hex): Promise<TransactionReceipt | null>;
}

const yes = (ret: Hex) => decodeWords(ret)[0] === 1n;

/** The chain through a JSON-RPC node (ethrex on chain 8141). */
export class RpcChain implements Chain {
  readonly rpc: RpcClient;
  constructor(rpc: RpcClient) {
    this.rpc = rpc;
  }

  blockNumber() {
    return this.rpc.blockNumber();
  }
  async codeHash(address: bigint): Promise<Hex> {
    return toHex(keccak256(fromHex(await this.rpc.code(address))));
  }
  async spent(pool: bigint, nullifier: bigint) {
    return yes(await this.rpc.call(pool, encodeCall("spent(uint256)", [nullifier])));
  }
  async knownPoolRoot(pool: bigint, root: bigint) {
    return yes(await this.rpc.call(pool, encodeCall("knownPoolRoot(uint256)", [root])));
  }
  async knownAssociationRoot(pool: bigint, root: bigint) {
    return yes(await this.rpc.call(pool, encodeCall("knownAssociationRoot(uint256)", [root])));
  }
  async verifyLeanSphincs(verifier: bigint, pkSeed: bigint, pkRoot: bigint, message: Uint8Array, signature: Uint8Array) {
    // verify(bytes32 pkSeed, bytes32 pkRoot, bytes32 message, bytes sig), the call the PQ paymaster makes.
    const data = concat(
      selector("verify(bytes32,bytes32,bytes32,bytes)"),
      word(pkSeed),
      word(pkRoot),
      message,
      word(0x80n),
      encodeBytesTail(signature),
    );
    try {
      return yes(await this.rpc.call(verifier, data));
    } catch {
      return false;
    }
  }
  simulate(raw: Uint8Array) {
    return this.rpc.simulateFrameTransaction(raw);
  }
  send(raw: Uint8Array) {
    return this.rpc.sendRawTransaction(raw);
  }
  receipt(hash: Hex) {
    return this.rpc.getReceipt(hash);
  }
}
