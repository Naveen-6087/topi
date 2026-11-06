// Standard EIP-1559 transfer on Chain 8141.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { RpcClient, addressOf, checksumAddress, parsePrivateKey, signEip1559 } from "../sdk/src/index.ts";
import { bigintOption, cli, main, rpc, settings, usageError } from "./lib/common.ts";

const USAGE = `
Usage: node scripts/transfer.ts --to ADDRESS --amount ETH [options]

Sends an EIP-1559 transfer on Chain 8141.

Options:
  --from-account NAME     Account name in data/wallet/accounts (e.g. account-1)
  --key FILE              ECDSA key file (default: from account or .secrets/deployer.key)
  --to ADDRESS            Recipient address
  --amount ETH            Amount in ETH (e.g. 0.1) or wei (with --wei)
  --wei                   Specify amount directly in wei
  --send                  Broadcast transaction (default: simulate/show details only)
  -h, --help              Show this help
`;

main(async () => {
  const { values } = cli(USAGE, {
    "from-account": { type: "string" },
    key: { type: "string" },
    to: { type: "string" },
    amount: { type: "string" },
    wei: { type: "boolean", default: false },
    send: { type: "boolean", default: false },
  });

  if (!values.to) usageError(USAGE, "missing --to ADDRESS");
  if (!values.amount) usageError(USAGE, "missing --amount");

  let keyFile = values.key;
  if (!keyFile && values["from-account"]) {
    const accKey = join(settings.walletDir(), "accounts", values["from-account"], "account.key");
    if (existsSync(accKey)) keyFile = accKey;
  }
  if (!keyFile) keyFile = settings.deployerKey();
  if (!existsSync(keyFile)) {
    throw new Error(`key file not found: ${keyFile}`);
  }

  const privKey = parsePrivateKey(readFileSync(keyFile, "utf8"));
  const senderAddr = checksumAddress(addressOf(privKey));
  const client = rpc();

  const balance = await client.balance(senderAddr);
  let valueWei: bigint;
  if (values.wei) {
    valueWei = BigInt(values.amount);
  } else {
    valueWei = BigInt(Math.floor(parseFloat(values.amount) * 1e18));
  }

  console.log(`From:    ${senderAddr}`);
  console.log(`To:      ${values.to}`);
  console.log(`Amount:  ${Number(valueWei) / 1e18} ETH (${valueWei} wei)`);
  console.log(`Balance: ${Number(balance) / 1e18} ETH`);

  if (balance < valueWei) {
    throw new Error(`insufficient balance: need ${Number(valueWei) / 1e18} ETH, have ${Number(balance) / 1e18} ETH`);
  }

  if (!values.send) {
    console.log("\n[Dry run] Pass --send to broadcast to Chain 8141.");
    return;
  }

  const fees = await client.feeCaps();
  const raw = signEip1559(
    {
      chainId: await client.chainId(),
      nonce: await client.transactionCount(senderAddr),
      maxPriorityFee: fees.maxPriorityFee,
      maxFee: fees.maxFee,
      gasLimit: 300_000n,
      to: BigInt(values.to),
      value: valueWei,
      data: new Uint8Array(0),
    },
    privKey,
  );

  console.log("\nBroadcasting transaction...");
  const txHash = await client.sendRawTransaction(raw);
  console.log(`Tx hash: ${txHash}`);
  console.log("Waiting for confirmation on Chain 8141...");
  const receipt = await client.waitForReceipt(txHash);
  console.log(`Confirmed in block ${receipt.blockNumber}! Status: ${receipt.status}`);
  console.log(`Explorer: https://dora.privacy.ethrex.xyz/tx/${txHash}`);
});
