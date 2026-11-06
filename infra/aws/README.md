# AWS deployment

Four mixnet nodes in four regions, so no single jurisdiction or data centre sees a packet's whole path.

| Role | Region | Public ports |
|---|---|---|
| gateway | ap-south-1 (Mumbai) | 47101 relay, 47201 rate limiter, 47250 reply store |
| mix | eu-central-1 (Frankfurt) | 47102 |
| exit | us-east-1 (N. Virginia) | 47103 |
| reply relay | ap-southeast-1 (Singapore) | 47104 |

Every node also serves read-only counters on 47300. Instances are t3.micro running Ubuntu 24.04, with
CPU credits pinned to `standard` (no burst billing) and 8 GB gp3 root volumes. Every resource is tagged
`Project = <deployment name>` (default `topi`, set `DEPLOYMENT_NAME` in `.env`).

## Use

```bash
aws login                                   # once
bash infra/aws/deploy.sh audit              # anything in the account that bills, in every region
bash infra/aws/deploy.sh launch             # all of the next four steps in one, for demos
bash infra/aws/build-linux.sh               # the Linux node binary (inside WSL on Windows)
bash infra/aws/deploy.sh up                 # terraform apply
bash infra/aws/deploy.sh provision          # keys, topology, binary and service on every node
bash infra/aws/deploy.sh ping               # a round trip through all four regions
bash infra/aws/deploy.sh stats              # live counters
bash infra/aws/deploy.sh fetch-queue        # the exit's queued spends, for the exit prover
bash infra/aws/deploy.sh down               # terraform destroy, then confirm nothing is left
```

The console's Mixnet map tab shows these nodes on the world map once they are up. State lives in
`data/aws` (nodes, topology, fetched queue, Linux binary); SSH and node keys in `.secrets/aws`.

**Cost.** About $0.07 per hour while up. Destroy after each session.

**Access.** SSH (key-only) and the stats port accept any source by default, because mobile and venue
networks change the public address, sometimes per destination port. On a fixed network restrict them
with `OPERATOR_CIDR=x.x.x.x/32` in `.env`.

## Exit prover

`prover.sh` runs the exit's leanVM recursion on one large instance (default `c7i.8xlarge`, 32 vCPU and
64 GiB, spot), started for proving and destroyed afterwards:

```bash
bash infra/aws/prover.sh up        # launch, wait for SSH (about 1 minute)
bash infra/aws/prover.sh setup     # toolchain and provers on it (about 6 minutes, once per instance)
bash infra/aws/prover.sh prove A.tgz B.tgz OUT_DIR   # one recursion there; writes OUT_DIR/calldata.bin here
EXIT_PROVER=aws node scripts/exit-prover.ts --queue data/local/spend-queue --mempool http://127.0.0.1:47500/rpc
bash infra/aws/prover.sh status    # instance, uptime, spot price
bash infra/aws/prover.sh ssh       # a shell on it
bash infra/aws/prover.sh down      # terminate it, remove its security group and key pair
```

The instance runs in ap-south-1 by default (`PROVER_REGION`, `PROVER_INSTANCE_TYPE`, `PROVER_SPOT`).
Spot costs about 0.47 USD per hour, so one settle's recursion costs a few cents.

`EXIT_PROVER=aws` in `.env` sends the console's exit prover (Wallet tab, `r`) there too. SSH listens
on 22 and 443, since some networks block 22 towards cloud addresses, and is open to any address by
default (`PROVER_SSH_CIDR` restricts it) because carrier-grade NAT sends traffic out from a pool of
addresses; the instance takes only the key in `.secrets/aws` and lives only while proving. A new account's
default vCPU quota (32 for on-demand and for spot) fits one such instance.

Two notes to a mined settle with `EXIT_PROVER=aws`:

| Step | Time |
|---|---|
| Deposits, witnesses, client proofs, mixnet sends | about 85 to 90 s |
| Recursion of the pair, warm instance | about 151 s (210 to 332 s on a fresh one; about 1,150 s and 12 GB on a laptop) |
| Settle through the alt mempool, mined | about 20 s |
| End to end | about 216 to 301 s |

## Notes

- `build-linux.sh` pins a portable CPU target (x86-64-v3 with AES and CLMUL), which every t3 instance
  supports. A binary tuned for the build machine's CPU can fail on the instances with an illegal
  instruction.
- Uploads are gzipped and retried; provisioning fails loudly if a node's upload or service start fails.
- A message that straddles a rate-limit epoch boundary keeps its continuation budget through the next
  epoch.

## Measured

| Check | Result |
|---|---|
| Ping round trip through all four regions | 3.8 to 10 s (the first in an epoch includes the ticket proof) |
| 1.3 MB spend (about 280 packets, one rate-limit token) | queued at the exit in 68 to 78 s over a mobile uplink |
| Two spends from leanSPHINCS-account deposits, through AWS, proven and settled | settles in blocks 557893, 558272 and 612907, about 16.3M gas each |
| Teardown | all resources destroyed; the account audit finds nothing billable |
