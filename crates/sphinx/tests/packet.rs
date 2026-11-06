use std::collections::HashSet;
use topi_sphinx::*;

fn route() -> (Vec<DecapsulationKey>, Vec<EncapsulationKey>) {
    let mut sks = vec![];
    let mut pks = vec![];
    for _ in 0..HOPS {
        let (sk, pk) = keygen();
        // Round-trip the public key through bytes, as the topology does.
        let pk = pk_from_bytes(&pk_bytes(&pk));
        sks.push(sk);
        pks.push(pk);
    }
    (sks, pks)
}

fn routing() -> Vec<Routing> {
    vec![
        Routing { is_exit: false, next: 2, delay_ms: 120 },
        Routing { is_exit: false, next: 3, delay_ms: 80 },
        Routing { is_exit: true, next: 99, delay_ms: 0 },
    ]
}

#[test]
fn three_hops_deliver_and_keep_size() {
    let (sks, pks) = route();
    let msg = br#"{"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":["0xabc","latest"]}"#;
    let mut pkt = build_packet(&pks, &routing(), msg);
    assert_eq!(pkt.len(), PACKET_SIZE);
    for (i, sk) in sks.iter().enumerate() {
        let mut seen = HashSet::new();
        let p = process(sk, &pkt, &mut seen).expect("hop ok");
        assert_eq!(p.routing, routing()[i]);
        if p.routing.is_exit {
            assert_eq!(decode_payload(&p.packet).unwrap(), msg.to_vec());
            return;
        }
        assert_eq!(p.packet.len(), PACKET_SIZE, "size invariant at hop {i}");
        pkt = p.packet;
    }
    panic!("never reached exit");
}

#[test]
fn replay_is_rejected() {
    let (sks, pks) = route();
    let pkt = build_packet(&pks, &routing(), b"hi");
    let mut seen = HashSet::new();
    assert!(process(&sks[0], &pkt, &mut seen).is_ok());
    assert_eq!(process(&sks[0], &pkt, &mut seen).err(), Some(HopError::Replay));
}

#[test]
fn tampered_header_fails_mac() {
    let (sks, pks) = route();
    let mut pkt = build_packet(&pks, &routing(), b"hi");
    pkt[CT + MAC + 5] ^= 1;
    let mut seen = HashSet::new();
    assert_eq!(process(&sks[0], &pkt, &mut seen).err(), Some(HopError::BadMac));
}

#[test]
fn tampered_payload_detected_at_exit() {
    let (sks, pks) = route();
    let mut pkt = build_packet(&pks, &routing(), b"hello");
    pkt[HEADER + 40] ^= 1;
    for sk in &sks {
        let mut seen = HashSet::new();
        let p = process(sk, &pkt, &mut seen).unwrap();
        if p.routing.is_exit {
            assert!(decode_payload(&p.packet).is_none());
            return;
        }
        pkt = p.packet;
    }
}

#[test]
fn wrong_node_cannot_process() {
    let (_sks, pks) = route();
    let (other, _) = keygen();
    let pkt = build_packet(&pks, &routing(), b"hi");
    let mut seen = HashSet::new();
    assert!(process(&other, &pkt, &mut seen).is_err());
}

#[test]
fn surb_reply_round_trip() {
    let (sks, pks) = route();
    let surb = build_surb(&pks, &routing());
    let reply = b"0x1bc16d674ec80000";
    let mut pkt = reply_packet(&surb.header, reply);
    assert_eq!(pkt.len(), PACKET_SIZE);
    for sk in &sks {
        let mut seen = HashSet::new();
        let p = process(sk, &pkt, &mut seen).unwrap();
        if p.routing.is_exit {
            assert_eq!(open_reply(&surb.keys, p.packet).unwrap(), reply.to_vec());
            return;
        }
        pkt = p.packet;
    }
    panic!("no exit");
}

#[test]
fn cover_and_real_packets_are_same_size() {
    let (_sks, pks) = route();
    let real = build_packet(&pks, &routing(), b"eth_sendFrameTransaction ...");
    let cover = cover_packet(&pks, &routing());
    assert_eq!(real.len(), cover.len());
}

#[test]
fn print_sizes() {
    println!("CT={CT} ENTRY={ENTRY} HEADER={HEADER} PAYLOAD={PAYLOAD} PACKET={PACKET_SIZE}");
}
