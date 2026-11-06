// Send one call from the leanSPHINCS PQ account as an EIP-8141 frame transaction:
//   frame 0  VERIFY  account  verifySelector   flags 3 (execution + payment)
//   frame 1  SENDER  target   data, value      (the account is the caller)
// signed by one ARBITRARY entry: the leanSPHINCS witness over the signature hash. No ECDSA key.
import { addArbitraryWitness, buildAccountTransaction, encodeRawHex, fromHex } from "../sdk/src/index.ts";
import { bigintOption, cli, loadAccount, lskSigner, main, rpc, settings, usageError } from "./lib/common.ts";
import { describeReceipt, summarizeSimulation } from "./lib/pool-ops.ts";

const USAGE = `
Usage: node scripts/account-send.ts --target ADDRESS [options]

Options:
  --target ADDRESS        Call target
  --value WEI             Value to send (default: 0)
  --data HEX              Calldata (default: empty)
  --gas N                 SENDER frame execution limit (default: 200000)
  --state N               SENDER frame state budget (default: 200000)
  --account FILE          PQ account record (default: ACCOUNT_DEPLOYMENT or deployments/8141/account.json)
  --pq-key FILE           leanSPHINCS key file (default: ACCOUNT_PQ_KEY)
  --corrupt               Flip one witness bit (negative test)
  --wrong-key FILE        Sign with another leanSPHINCS key (negative test)
  --send                  Broadcast after simulating (otherwise simulate only)
  -h, --help              Show this help
`;

main(async () => {
  const { values } = cli(USAGE, {
    target: { type: "string" },
    value: { type: "string" },
    data: { type: "string" },
    gas: { type: "string" },
    state: { type: "string" },
    account: { type: "string" },
    "pq-key": { type: "string" },
    corrupt: { type: "boolean", default: false },
    "wrong-key": { type: "string" },
    send: { type: "boolean", default: false },
  });
  if (!values.target || !/^0x[0-9a-fA-F]{1,40}$/.test(values.target)) usageError(USAGE, "--target ADDRESS is required");
  const account = loadAccount(values.account);
  const client = rpc();
  const tx = buildAccountTransaction({
    account: account.account,
    verifySelector: account.verifySelector,
    chainId: await client.chainId(),
    fees: await client.feeCaps(),
    call: {
      target: BigInt(values.target),
      value: bigintOption(values.value, 0n, "--value"),
      data: fromHex(values.data ?? "0x"),
      gasLimit: bigintOption(values.gas, 200_000n, "--gas"),
      stateLimit: bigintOption(values.state, 200_000n, "--state"),
    },
  });
  await addArbitraryWitness(tx, lskSigner(values["wrong-key"] ?? values["pq-key"] ?? settings.accountPqKey()));
  if (values.corrupt) tx.signatures[0].signature[100] ^= 1;
  const raw = encodeRawHex(tx);
  const sim = await client.simulateFrameTransaction(raw);
  console.log(`account tx ${(raw.length - 2) / 2} bytes; simulate: ${summarizeSimulation(sim)}`);
  if (!values.send) return;
  const receipt = await client.waitForReceipt(await client.sendRawTransaction(raw));
  console.log(describeReceipt(receipt));
});
