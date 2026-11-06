#!/usr/bin/env bash
set -e

DEMO_DIR="demo_run"
rm -rf "$DEMO_DIR"
mkdir -p "$DEMO_DIR/keys" "$DEMO_DIR/queue"

echo "=========================================="
echo "1. Generating Keys for the 4 Mixnet Nodes"
echo "=========================================="
target/release/topi-node keygen "$DEMO_DIR/keys/gateway.key" > /dev/null
target/release/topi-node keygen "$DEMO_DIR/keys/mix.key" > /dev/null
target/release/topi-node keygen "$DEMO_DIR/keys/exit.key" > /dev/null
target/release/topi-node keygen "$DEMO_DIR/keys/reply-relay.key" > /dev/null
echo "✓ Generated gateway.key, mix.key, exit.key, reply-relay.key"

echo ""
echo "=========================================="
echo "2. Registering Client Identity & Membership"
echo "=========================================="
target/release/topi-client register --identity "$DEMO_DIR/member.id" --members "$DEMO_DIR/members.txt"
echo "✓ Member commitment added to members.txt"

echo ""
echo "=========================================="
echo "3. Creating Network Topology (topology.json)"
echo "=========================================="
target/release/topi-node topology "$DEMO_DIR/topology.json" \
  --members "$DEMO_DIR/members.txt" \
  --keys "$DEMO_DIR/keys" \
  --hosts 127.0.0.1 127.0.0.1 127.0.0.1 127.0.0.1

echo ""
echo "=========================================="
echo "4. Launching the 4 Mixnet Nodes"
echo "=========================================="
target/release/topi-node run gateway --topology "$DEMO_DIR/topology.json" --key "$DEMO_DIR/keys/gateway.key" --bind 127.0.0.1 --stats-port 47300 --metrics-port 47400 > "$DEMO_DIR/gateway.log" 2>&1 &
PID_GW=$!

target/release/topi-node run mix --topology "$DEMO_DIR/topology.json" --key "$DEMO_DIR/keys/mix.key" --bind 127.0.0.1 --stats-port 47310 --metrics-port 47410 --batch-ms 300 > "$DEMO_DIR/mix.log" 2>&1 &
PID_MIX=$!

target/release/topi-node run exit --topology "$DEMO_DIR/topology.json" --key "$DEMO_DIR/keys/exit.key" --bind 127.0.0.1 --stats-port 47320 --metrics-port 47420 --spend-queue "$DEMO_DIR/queue" > "$DEMO_DIR/exit.log" 2>&1 &
PID_EXIT=$!

target/release/topi-node run reply-relay --topology "$DEMO_DIR/topology.json" --key "$DEMO_DIR/keys/reply-relay.key" --bind 127.0.0.1 --stats-port 47330 --metrics-port 47430 > "$DEMO_DIR/reply-relay.log" 2>&1 &
PID_RR=$!

cleanup() {
    echo ""
    echo "Shutting down nodes..."
    kill $PID_GW $PID_MIX $PID_EXIT $PID_RR 2>/dev/null || true
}
trap cleanup EXIT

echo "✓ Gateway PID: $PID_GW"
echo "✓ Mix PID: $PID_MIX"
echo "✓ Exit PID: $PID_EXIT"
echo "✓ Reply-Relay PID: $PID_RR"
sleep 1

echo ""
echo "=========================================="
echo "5. Sending Anonymous Ping Round-Trip"
echo "=========================================="
echo "Sending ping through: Client -> Gateway -> Mix -> Exit -> Reply-Relay -> Mix -> Gateway -> Client..."
target/release/topi-client ping --topology "$DEMO_DIR/topology.json" --identity "$DEMO_DIR/member.id"

echo ""
echo "=========================================="
echo "6. Submitting a Private Spend Transaction"
echo "=========================================="
# Create dummy 82 fields and dummy proof
printf '{"fields": [%s]}
' "$(seq -s, 0 81)" > "$DEMO_DIR/public.json"
head -c 25000 /dev/urandom > "$DEMO_DIR/client_proof.tgz"

echo "Submitting 25 KB transaction payload through the mixnet..."
target/release/topi-client spend --topology "$DEMO_DIR/topology.json" --identity "$DEMO_DIR/member.id" --proof "$DEMO_DIR/client_proof.tgz" --public "$DEMO_DIR/public.json"

echo ""
echo "=========================================="
echo "7. Inspecting Exit Node Spend Queue"
echo "=========================================="
ls -lh "$DEMO_DIR/queue"
echo "Content of header.json:"
cat "$DEMO_DIR/queue/"*.json
echo ""

echo "=========================================="
echo "Demo finished successfully!"
echo "=========================================="
