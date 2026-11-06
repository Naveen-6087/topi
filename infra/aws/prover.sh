#!/usr/bin/env bash
# The exit prover on AWS: one large instance, started for proving and destroyed afterwards. The leanVM
# recursion uses every core it gets, so a 32-core machine proves a pair several times faster than a laptop.
#
#   up       launch the instance (spot by default) and wait for SSH
#   setup    install the toolchain and build the provers on it (once per instance, about 6 minutes)
#   prove A.tgz B.tgz OUT_DIR   recurse two client proofs there; writes OUT_DIR/calldata.bin here
#   status   what is running and what it costs per hour
#   ssh      a shell on it
#   down     terminate it and remove its security group and key pair (nothing left to bill)
#
# Settings (.env): PROVER_REGION (default ap-south-1), PROVER_INSTANCE_TYPE (default c7i.8xlarge,
# 32 vCPU / 64 GiB), PROVER_SPOT (default true). Uses the AWS CLI's signed-in session (aws login).
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
if [ -f "$ROOT/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . <(tr -d '\r' <"$ROOT/.env")
  set +a
fi
SEC=$ROOT/.secrets/aws
STATE=$ROOT/data/aws/prover
REGION=${PROVER_REGION:-ap-south-1}
TYPE=${PROVER_INSTANCE_TYPE:-c7i.8xlarge}
SPOT=${PROVER_SPOT:-true}
NAME=topi-prover
export AWS_REGION=$REGION AWS_DEFAULT_REGION=$REGION
# Git Bash on Windows: pass "/aws/service/..." and similar arguments to aws.exe unchanged.
export MSYS_NO_PATHCONV=1
mkdir -p "$STATE"

say() {
  printf '[prover] %s\n' "$*" >&2
  mkdir -p "$ROOT/logs"
  printf '%s aws-prover %s\n' "$(date +%s)" "$*" >>"$ROOT/logs/pipeline.log"
}
q() { aws "$@" --output text; }
# SSH on 443 as well as 22: some networks block port 22 towards cloud addresses.
PORT=${PROVER_SSH_PORT:-443}
SSH_OPTS=(-i "$SEC/id_ed25519" -o StrictHostKeyChecking=accept-new -o "UserKnownHostsFile=$SEC/known_hosts"
  -o ConnectTimeout=10 -o BatchMode=yes -o ServerAliveInterval=30 -o "Port=$PORT")
USER_DATA='#!/bin/bash
mkdir -p /etc/systemd/system/ssh.socket.d
printf "[Socket]\nListenStream=\nListenStream=0.0.0.0:22\nListenStream=0.0.0.0:443\n" >/etc/systemd/system/ssh.socket.d/listen.conf
systemctl daemon-reload
systemctl restart ssh.socket'
ip() { cat "$STATE/ip" 2>/dev/null || { echo "no prover running: infra/aws/prover.sh up" >&2; exit 1; }; }
remote() { ssh "${SSH_OPTS[@]}" "ubuntu@$(ip)" "$@"; }

# SSH from where (PROVER_SSH_CIDR). Default: anywhere, because many ISPs send traffic out through a
# pool of addresses (carrier-grade NAT) so no single /32 works. The instance takes only our key (no
# passwords) and lives only while proving. On a fixed address, set PROVER_SSH_CIDR=<your ip>/32.
allow() {
  local sg=$1 cidr=${PROVER_SSH_CIDR:-0.0.0.0/0} port
  for port in 22 443; do
    aws ec2 authorize-security-group-ingress --group-id "$sg" --protocol tcp --port "$port" --cidr "$cidr" >/dev/null 2>&1 || true
  done
}
allow_me() {
  [ -f "$STATE/sg" ] && allow "$(cat "$STATE/sg")"
  return 0
}

ensure_key() {
  [ -f "$SEC/id_ed25519" ] || { mkdir -p "$SEC"; ssh-keygen -q -t ed25519 -N "" -f "$SEC/id_ed25519"; }
  q ec2 describe-key-pairs --key-names "$NAME" >/dev/null 2>&1 ||
    q ec2 import-key-pair --key-name "$NAME" --public-key-material "fileb://$(cygpath -m "$SEC/id_ed25519.pub" 2>/dev/null || echo "$SEC/id_ed25519.pub")" >/dev/null
}

ensure_sg() {
  local vpc sg cidr
  vpc=$(q ec2 describe-vpcs --filters Name=isDefault,Values=true --query 'Vpcs[0].VpcId')
  sg=$(q ec2 describe-security-groups --filters "Name=group-name,Values=$NAME" "Name=vpc-id,Values=$vpc" \
    --query 'SecurityGroups[0].GroupId')
  if [ "$sg" = "None" ] || [ -z "$sg" ]; then
    sg=$(q ec2 create-security-group --group-name "$NAME" --description "topi exit prover: ssh from the operator" \
      --vpc-id "$vpc" --query GroupId)
  fi
  allow "$sg"
  echo "$sg"
}

cmd_up() {
  if [ -f "$STATE/instance" ]; then
    local st
    st=$(q ec2 describe-instances --instance-ids "$(cat "$STATE/instance")" --query 'Reservations[0].Instances[0].State.Name' 2>/dev/null || echo gone)
    if [ "$st" = "running" ] || [ "$st" = "pending" ]; then say "already up: $(cat "$STATE/instance") ($st)"; return; fi
  fi
  ensure_key
  local sg ami market=() id addr
  sg=$(ensure_sg)
  ami=$(q ssm get-parameter --name /aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id \
    --query Parameter.Value)
  if [ "$SPOT" = "true" ]; then
    market=(--instance-market-options '{"MarketType":"spot","SpotOptions":{"SpotInstanceType":"one-time","InstanceInterruptionBehavior":"terminate"}}')
  fi
  say "launching $TYPE ($([ "$SPOT" = true ] && echo spot || echo on-demand)) in $REGION"
  id=$(q ec2 run-instances --image-id "$ami" --instance-type "$TYPE" --key-name "$NAME" --security-group-ids "$sg" \
    "${market[@]}" --user-data "$USER_DATA" \
    --block-device-mappings '[{"DeviceName":"/dev/sda1","Ebs":{"VolumeSize":60,"VolumeType":"gp3","DeleteOnTermination":true}}]' \
    --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=$NAME},{Key=Project,Value=$NAME}]" \
    "ResourceType=volume,Tags=[{Key=Project,Value=$NAME}]" \
    --query 'Instances[0].InstanceId')
  echo "$id" >"$STATE/instance"
  echo "$sg" >"$STATE/sg"
  date +%s >"$STATE/started"
  aws ec2 wait instance-running --instance-ids "$id"
  addr=$(q ec2 describe-instances --instance-ids "$id" --query 'Reservations[0].Instances[0].PublicIpAddress')
  echo "$addr" >"$STATE/ip"
  for _ in $(seq 1 40); do remote true 2>/dev/null && break; sleep 5; done
  remote true
  say "up: $id at $addr"
}

cmd_setup() {
  allow_me
  say "installing the toolchain on $(ip)"
  remote 'set -e
    sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config libssl-dev git curl python3 >/dev/null
    [ -x ~/.cargo/bin/cargo ] || curl -fsSL https://sh.rustup.rs | sh -s -- -y -q --profile minimal >/dev/null
    [ -x ~/.foundry/bin/forge ] || { curl -fsSL https://foundry.paradigm.xyz | bash >/dev/null; ~/.foundry/bin/foundryup >/dev/null 2>&1; }
    mkdir -p ~/topi'
  say "copying prover/ and the terminal manifest"
  tar -C "$ROOT" -czf - prover contracts/test/fixtures/terminal/manifest.json | remote 'tar -xzf - -C ~/topi'
  say "building the provers (spartan-whir, leanVM, sol-whir-p3)"
  remote 'set -e; export PATH=$HOME/.cargo/bin:$HOME/.foundry/bin:$PATH; cd ~/topi && bash prover/setup.sh'
  say "setup done; the first prove also compiles the leanVM guest"
}

cmd_prove() {
  local a=$1 b=$2 out=$3 job
  [ -f "$a" ] && [ -f "$b" ] || { echo "usage: prover.sh prove A.tgz B.tgz OUT_DIR" >&2; exit 2; }
  allow_me
  job="/tmp/job-$(date +%s)-$$"
  remote "mkdir -p $job"
  scp -q "${SSH_OPTS[@]}" "$a" "ubuntu@$(ip):$job/a.tgz"
  scp -q "${SSH_OPTS[@]}" "$b" "ubuntu@$(ip):$job/b.tgz"
  say "recursion on $(ip) ($TYPE)"
  remote "export PATH=\$HOME/.cargo/bin:\$HOME/.foundry/bin:\$PATH; cd ~/topi && bash prover/prove-exit.sh $job/a.tgz $job/b.tgz $job/out"
  mkdir -p "$out"
  scp -q "${SSH_OPTS[@]}" "ubuntu@$(ip):$job/out/calldata.bin" "$out/calldata.bin"
  remote "rm -rf $job"
  say "calldata: $out/calldata.bin"
}

cmd_status() {
  [ -f "$STATE/instance" ] || { echo "no prover instance"; return; }
  local id st since
  id=$(cat "$STATE/instance")
  st=$(q ec2 describe-instances --instance-ids "$id" --query 'Reservations[0].Instances[0].State.Name' 2>/dev/null || echo gone)
  since=$(($(date +%s) - $(cat "$STATE/started" 2>/dev/null || date +%s)))
  echo "$id $TYPE $st, up $((since / 60)) min, ip $(cat "$STATE/ip" 2>/dev/null)"
  if [ "$SPOT" = "true" ]; then
    echo "spot price now: \$$(q ec2 describe-spot-price-history --instance-types "$TYPE" --product-descriptions Linux/UNIX \
      --max-items 1 --query 'SpotPriceHistory[0].SpotPrice' | head -1)/hour"
  fi
}

cmd_down() {
  if [ -f "$STATE/instance" ]; then
    local id
    id=$(cat "$STATE/instance")
    aws ec2 terminate-instances --instance-ids "$id" >/dev/null 2>&1 || true
    aws ec2 wait instance-terminated --instance-ids "$id" 2>/dev/null || true
    say "terminated $id"
  fi
  local sg
  sg=$(cat "$STATE/sg" 2>/dev/null || true)
  [ -n "$sg" ] && aws ec2 delete-security-group --group-id "$sg" >/dev/null 2>&1 || true
  aws ec2 delete-key-pair --key-name "$NAME" >/dev/null 2>&1 || true
  local left
  left=$(q ec2 describe-instances --filters "Name=tag:Project,Values=$NAME" \
    Name=instance-state-name,Values=pending,running,stopping,stopped --query 'length(Reservations[].Instances[])')
  rm -f "$STATE/instance" "$STATE/ip" "$STATE/sg" "$STATE/started"
  say "$REGION: $left prover instance(s) left; nothing billing"
}

case ${1:-} in
  up) cmd_up ;;
  setup) cmd_setup ;;
  prove) shift; cmd_prove "$@" ;;
  status) cmd_status ;;
  ssh) shift; allow_me; remote "$@" ;;
  down) cmd_down ;;
  *) sed -n '2,13p' "$0"; exit 2 ;;
esac
