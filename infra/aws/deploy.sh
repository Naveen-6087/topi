#!/usr/bin/env bash
# The mixnet on AWS, end to end. Needs bash, the AWS CLI (signed in: `aws login`), terraform and ssh.
#   whoami       the AWS identity the CLI is signed in as
#   audit        everything in every enabled region that can bill (instances, IPs, NAT, LBs, databases, ...)
#   up           create the four nodes (terraform apply)
#   provision    node keys, topology, binary and service on every node; starts them
#   ping         a round trip through all four regions
#   stats        each node's live counters, one JSON line per node
#   fetch-queue  copy the exit's queued pool spends to data/aws/spend-queue for the exit prover
#   down         terraform destroy, then confirm nothing tagged with the deployment name is left
#   launch       everything for a demo: identity, Linux binary if missing, up, provision, ping
# Secrets (SSH key, node keys) live in .secrets/aws (git-ignored); state in data/aws.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && (pwd -W 2>/dev/null || pwd))
[ -f "$ROOT/.env" ] && { set -a; . "$ROOT/.env"; set +a; }
NAME=${DEPLOYMENT_NAME:-topi}
TF=$ROOT/infra/aws
SEC=$ROOT/.secrets/aws
STATE=$ROOT/data/aws
LINUX_BIN=$STATE/bin/topi-node
EXE=$( [ -f "$ROOT/target/release/topi-node.exe" ] && echo .exe || echo "")
# The console passes TOPI_BIN_DIR (where topi, topi-node and topi-client are installed).
BIN_DIR=${TOPI_BIN_DIR:-$ROOT/target/release}
[ -f "$BIN_DIR/topi-node.exe" ] && EXE=.exe
NODE_BIN=$BIN_DIR/topi-node$EXE
CLIENT_BIN=$BIN_DIR/topi-client$EXE
MEMBERS=${MEMBERS_FILE:-$ROOT/data/members.txt}
IDENTITY=${MEMBER_IDENTITY:-$ROOT/.secrets/member.id}
REGIONS=(ap-south-1 eu-central-1 us-east-1 ap-southeast-1)
ROLES=(gateway mix exit reply-relay)
mkdir -p "$STATE" "$ROOT/logs"
export MSYS_NO_PATHCONV=1
export AWS_REGION=${AWS_REGION:-ap-south-1} AWS_DEFAULT_REGION=${AWS_DEFAULT_REGION:-ap-south-1}

say() {
  printf '[deploy] %s\n' "$*"
  printf '%s aws %s\n' "$(date +%s)" "$*" >>"$ROOT/logs/pipeline.log"
}

# Hand the CLI's signed-in session (aws login, SSO or a profile) to terraform without writing it to disk.
creds() {
  eval "$(aws configure export-credentials --format env)"
}

SSH_OPTS=(-i "$SEC/id_ed25519" -o StrictHostKeyChecking=accept-new -o "UserKnownHostsFile=$SEC/known_hosts" -o ConnectTimeout=8 -o BatchMode=yes)

tf() {
  terraform -chdir="$TF" "$@" -var "name=$NAME"
}

# role ip region instance-id, one line per node
nodes() {
  terraform -chdir="$TF" output -json nodes | node -e '
    let s = ""; process.stdin.on("data", d => s += d).on("end", () => {
      for (const n of JSON.parse(s)) console.log(n.role, n.public_ip, n.region, n.instance_id);
    })'
}

ip_of() { awk -v r="$1" '$1 == r { print $2 }' "$STATE/nodes.txt"; }

cmd_whoami() {
  aws sts get-caller-identity --output json
}

cmd_audit() {
  local regions
  regions=$(aws ec2 describe-regions --query 'Regions[].RegionName' --output text)
  say "scanning $(echo "$regions" | wc -w) regions for anything that bills"
  for r in $regions; do
    (
      q() { aws "$@" --region "$r" --output text 2>/dev/null | grep -v '^None$' | grep -v '^$' || true; }
      found() { while read -r id rest; do printf 'FOUND\t%s\t%s\t%s\t%s\n' "$r" "$1" "$id" "$rest"; done; }
      q ec2 describe-instances --filters Name=instance-state-name,Values=pending,running,stopping,stopped \
        --query 'Reservations[].Instances[].[InstanceId,InstanceType,State.Name]' | found ec2-instance
      q ec2 describe-addresses --query 'Addresses[].[AllocationId,PublicIp,AssociationId]' | found elastic-ip
      q ec2 describe-nat-gateways --filter Name=state,Values=available,pending --query 'NatGateways[].[NatGatewayId,VpcId]' | found nat-gateway
      q ec2 describe-volumes --query 'Volumes[].[VolumeId,Size,State]' | found ebs-volume
      q ec2 describe-snapshots --owner-ids self --query 'Snapshots[].[SnapshotId,VolumeSize]' | found ebs-snapshot
      q ec2 describe-vpc-endpoints --filters Name=vpc-endpoint-type,Values=Interface --query 'VpcEndpoints[].[VpcEndpointId,ServiceName]' | found vpc-endpoint
      q elbv2 describe-load-balancers --query 'LoadBalancers[].[LoadBalancerName,Type]' | found load-balancer
      q elb describe-load-balancers --query 'LoadBalancerDescriptions[].[LoadBalancerName]' | found classic-load-balancer
      q rds describe-db-instances --query 'DBInstances[].[DBInstanceIdentifier,DBInstanceClass,DBInstanceStatus]' | found rds-instance
      q rds describe-db-clusters --query 'DBClusters[].[DBClusterIdentifier,Status]' | found rds-cluster
      q eks list-clusters --query 'clusters[]' | tr '\t' '\n' | found eks-cluster
      q elasticache describe-cache-clusters --query 'CacheClusters[].[CacheClusterId,CacheNodeType]' | found elasticache
      q sagemaker list-endpoints --query 'Endpoints[].[EndpointName,EndpointStatus]' | found sagemaker-endpoint
      q sagemaker list-notebook-instances --query 'NotebookInstances[].[NotebookInstanceName,NotebookInstanceStatus]' | found sagemaker-notebook
      q lightsail get-instances --query 'instances[].[name,state.name]' | found lightsail
      q redshift describe-clusters --query 'Clusters[].[ClusterIdentifier,NodeType]' | found redshift
      q opensearch list-domain-names --query 'DomainNames[].[DomainName]' | found opensearch
    ) &
  done
  wait
  r=global
  aws s3api list-buckets --query 'Buckets[].[Name]' --output text 2>/dev/null | grep -v '^$' |
    while read -r b; do printf 'FOUND\t%s\ts3-bucket\t%s\t\n' "$r" "$b"; done || true
  aws route53 list-hosted-zones --query 'HostedZones[].[Id,Name]' --output text 2>/dev/null | grep -v '^$' |
    while read -r id n; do printf 'FOUND\t%s\troute53-zone\t%s\t%s\n' "$r" "$id" "$n"; done || true
  say "audit done"
}

cmd_up() {
  creds
  mkdir -p "$SEC"
  [ -f "$SEC/id_ed25519" ] || ssh-keygen -q -t ed25519 -N "" -C "$NAME-operator" -f "$SEC/id_ed25519"
  # Mobile and venue networks rotate the public address (carrier NAT may even use a different address per
  # destination port), so by default SSH (key-only) and the read-only stats port accept any source.
  # On a fixed network restrict them with OPERATOR_CIDR=x.x.x.x/32.
  local cidr=${OPERATOR_CIDR:-0.0.0.0/0}
  say "SSH (key-only) and stats reachable from $cidr"
  terraform -chdir="$TF" init -input=false -upgrade >/dev/null
  tf apply -input=false -auto-approve -var "operator_cidr=$cidr" -var "ssh_public_key=$(cat "$SEC/id_ed25519.pub")"
  nodes | tee "$STATE/nodes.txt"
  [ -f "$STATE/started" ] || date +%s >"$STATE/started"
  say "4 nodes up: Mumbai, Frankfurt, N. Virginia, Singapore"
}

cmd_provision() {
  [ -f "$LINUX_BIN" ] || { say "missing $LINUX_BIN: run infra/aws/build-linux.sh (in WSL on Windows)"; exit 1; }
  mkdir -p "$SEC/keys"
  for r in "${ROLES[@]}"; do
    [ -f "$SEC/keys/$r.key" ] || "$NODE_BIN" keygen "$SEC/keys/$r.key" >/dev/null
  done
  nodes >"$STATE/nodes.txt"
  "$NODE_BIN" topology "$STATE/topology.json" --members "$MEMBERS" --keys "$SEC/keys" \
    --hosts "$(ip_of gateway)" "$(ip_of mix)" "$(ip_of exit)" "$(ip_of reply-relay)" >/dev/null
  [ "$STATE/bin/topi-node.gz" -nt "$LINUX_BIN" ] || gzip -9 -k -f "$LINUX_BIN"
  local pids=()
  for r in "${ROLES[@]}"; do
    provision_node "$r" &
    pids+=($!)
  done
  local failed=0
  for p in "${pids[@]}"; do wait "$p" || failed=1; done
  [ "$failed" = 0 ] || { say "provisioning FAILED on at least one node"; exit 1; }
  say "provisioned: topology in data/aws/topology.json"
}

provision_node() {
  local r=$1 ip ok=""
  ip=$(ip_of "$r")
  for _ in $(seq 1 40); do ssh "${SSH_OPTS[@]}" "ubuntu@$ip" true 2>/dev/null && break; sleep 3; done
  for try in 1 2 3 4 5 6; do
    scp -q "${SSH_OPTS[@]}" "$STATE/bin/topi-node.gz" "$STATE/topology.json" "ubuntu@$ip:/tmp/" &&
      scp -q "${SSH_OPTS[@]}" "$SEC/keys/$r.key" "ubuntu@$ip:/tmp/node.key" && { ok=1; break; }
    say "$r: upload attempt $try failed, retrying"
    sleep 8
  done
  [ -n "$ok" ] || { say "$r ($ip) FAILED: upload"; return 1; }
  # shellcheck disable=SC2029 # the role is meant to expand here, before the command is sent
  ssh "${SSH_OPTS[@]}" "ubuntu@$ip" "set -e
    sudo systemctl stop mixnode 2>/dev/null || true
    gunzip -f /tmp/topi-node.gz
    sudo mkdir -p /opt/mixnode/spend-queue
    sudo mv /tmp/topi-node /tmp/topology.json /tmp/node.key /opt/mixnode/
    sudo chown -R ubuntu:ubuntu /opt/mixnode && chmod 600 /opt/mixnode/node.key && chmod +x /opt/mixnode/topi-node
    sudo tee /etc/systemd/system/mixnode.service >/dev/null <<UNIT
[Unit]
Description=Mixnet node ($r)
After=network-online.target
[Service]
User=ubuntu
WorkingDirectory=/opt/mixnode
ExecStart=/opt/mixnode/topi-node run $r --topology /opt/mixnode/topology.json --key /opt/mixnode/node.key --spend-queue /opt/mixnode/spend-queue
Restart=always
[Install]
WantedBy=multi-user.target
UNIT
    sudo systemctl daemon-reload && sudo systemctl enable --now mixnode >/dev/null 2>&1 && sudo systemctl restart mixnode
    sleep 4
    systemctl is-active mixnode" >/dev/null || { say "$r ($ip) FAILED: service did not start"; return 1; }
  say "$r ($ip) provisioned"
}

cmd_ping() {
  local out
  out=$("$CLIENT_BIN" ping --topology "$STATE/topology.json" --identity "$IDENTITY" --timeout 120)
  echo "$out"
  say "round trip through Mumbai, Frankfurt, N. Virginia, Singapore: $(echo "$out" | sed -E 's/.*"round_trip_ms":([0-9]+).*/\1/') ms"
}

cmd_stats() {
  while read -r role ip region _; do
    printf '%s %s %s ' "$role" "$region" "$ip"
    node -e '
      const s = require("net").connect(47300, process.argv[1]); let b = "";
      s.setTimeout(5000, () => { console.log("{\"up\":false}"); process.exit(); });
      s.on("data", d => b += d).on("end", () => console.log(b)).on("error", () => console.log("{\"up\":false}"));' "$ip"
  done <"$STATE/nodes.txt"
}

cmd_fetch_queue() {
  local q=$STATE/spend-queue
  mkdir -p "$q"
  scp -q "${SSH_OPTS[@]}" "ubuntu@$(ip_of exit):/opt/mixnode/spend-queue/*" "$q/" 2>/dev/null || true
  # spends the exit prover already settled or rejected stay handled
  for f in "$q"/done/*.json "$q"/rejected/*.json; do
    [ -f "$f" ] && rm -f "$q/$(basename "$f")" "$q/$(basename "$f" .json).tgz"
  done
  local n=0
  for f in "$q"/*.json; do [ -f "$f" ] && n=$((n + 1)); done
  say "$n new spend(s) in data/aws/spend-queue"
}

cmd_down() {
  creds
  if [ -f "$TF/terraform.tfstate" ]; then
    tf destroy -input=false -auto-approve -var "operator_cidr=0.0.0.0/32" -var "ssh_public_key=unused"
  fi
  rm -f "$STATE/nodes.txt" "$STATE/topology.json" "$STATE/started"
  cmd_verify_down
}

cmd_verify_down() {
  local left=0 n v
  for r in "${REGIONS[@]}"; do
    n=$(aws ec2 describe-instances --region "$r" --filters "Name=tag:Project,Values=$NAME" \
      Name=instance-state-name,Values=pending,running,stopping,stopped --query 'length(Reservations[].Instances[])' --output text)
    v=$(aws ec2 describe-volumes --region "$r" --filters "Name=tag:Project,Values=$NAME" --query 'length(Volumes)' --output text)
    say "$r: $n instance(s), $v volume(s) left"
    left=$((left + n + v))
  done
  if [ "$left" = 0 ]; then
    say "nothing left running: no further charges"
  else
    say "WARNING: $left resource(s) left"
    exit 1
  fi
}

# The whole network in one step, for demos: each stage stops the launch if it fails.
cmd_launch() {
  say "launch: identity, Linux binary, four nodes, provisioning, a round trip"
  cmd_whoami >/dev/null || { say "not signed in to AWS: run aws login first"; exit 1; }
  if [ ! -f "$LINUX_BIN" ]; then
    say "building the Linux node binary"
    case "$(uname -s)" in
      MINGW* | MSYS* | CYGWIN*)
        local drive=${ROOT%%:*}
        wsl -d "${WSL_DISTRO:-Ubuntu}" --exec bash "/mnt/${drive,,}${ROOT#*:}/infra/aws/build-linux.sh"
        ;;
      *) bash "$TF/build-linux.sh" ;;
    esac
  fi
  cmd_up
  cmd_provision
  cmd_ping
  say "launch complete: four nodes running; stop billing with: bash infra/aws/deploy.sh down"
}

case "${1:-}" in
  whoami) cmd_whoami ;;
  audit) cmd_audit ;;
  up) cmd_up ;;
  provision) cmd_provision ;;
  ping) cmd_ping ;;
  stats) cmd_stats ;;
  fetch-queue) cmd_fetch_queue ;;
  down) cmd_down ;;
  verify-down) cmd_verify_down ;;
  launch) cmd_launch ;;
  *)
    sed -n '2,12p' "$0"
    exit 1
    ;;
esac
