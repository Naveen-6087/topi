/**
 * EIP-1559 (type 0x02) transactions, for plain ECDSA accounts (deposits and deployments from an
 * externally owned account).
 */
import { address20, concat, keccak256, toBigint } from "./bytes.ts";
import { signHash } from "./ecdsa.ts";
import { rlpBytes, rlpInt, rlpList } from "./rlp.ts";

export interface Eip1559Transaction {
  chainId: bigint;
  nonce: bigint;
  maxPriorityFee: bigint;
  maxFee: bigint;
  gasLimit: bigint;
  /** null creates a contract. */
  to: bigint | null;
  value: bigint;
  data: Uint8Array;
}

function fields(tx: Eip1559Transaction): Uint8Array[] {
  return [
    rlpInt(tx.chainId),
    rlpInt(tx.nonce),
    rlpInt(tx.maxPriorityFee),
    rlpInt(tx.maxFee),
    rlpInt(tx.gasLimit),
    rlpBytes(tx.to === null ? new Uint8Array(0) : address20(tx.to)),
    rlpInt(tx.value),
    rlpBytes(tx.data),
    rlpList([]), // access list
  ];
}

/** keccak256(0x02 || rlp(unsigned fields)), what the sender signs. */
export function eip1559SigningHash(tx: Eip1559Transaction): Uint8Array {
  return keccak256(concat(Uint8Array.of(0x02), rlpList(fields(tx))));
}

/** The signed raw transaction: 0x02 || rlp([...fields, y_parity, r, s]). */
export function signEip1559(tx: Eip1559Transaction, key: Uint8Array): Uint8Array {
  const signature = signHash(eip1559SigningHash(tx), key);
  const yParity = BigInt(signature[0]);
  const r = toBigint(signature.subarray(1, 33));
  const s = toBigint(signature.subarray(33, 65));
  return concat(Uint8Array.of(0x02), rlpList([...fields(tx), rlpInt(yParity), rlpInt(r), rlpInt(s)]));
}
