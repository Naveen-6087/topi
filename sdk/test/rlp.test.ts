import assert from "node:assert/strict";
import { test } from "node:test";

import { fromHex, rlpBytes, rlpDecode, rlpEncode, rlpInt, rlpItems, rlpList, toHex, type RlpItem } from "../src/index.ts";

const utf8 = (s: string) => new TextEncoder().encode(s);

test("byte strings (Ethereum RLP reference vectors)", () => {
  assert.equal(toHex(rlpBytes(utf8("dog"))), "0x83646f67");
  assert.equal(toHex(rlpBytes(new Uint8Array(0))), "0x80");
  assert.equal(toHex(rlpBytes(Uint8Array.of(0x0f))), "0x0f");
  assert.equal(toHex(rlpBytes(Uint8Array.of(0x80))), "0x8180");
  const lorem = utf8("Lorem ipsum dolor sit amet, consectetur adipisicing elit");
  assert.equal(toHex(rlpBytes(lorem)).slice(0, 6), "0xb838");
});

test("integers", () => {
  assert.equal(toHex(rlpInt(0n)), "0x80");
  assert.equal(toHex(rlpInt(15n)), "0x0f");
  assert.equal(toHex(rlpInt(1024n)), "0x820400");
  assert.equal(toHex(rlpInt(127n)), "0x7f");
  assert.equal(toHex(rlpInt(128n)), "0x8180");
  assert.throws(() => rlpInt(-1n));
});

test("lists", () => {
  assert.equal(toHex(rlpList([])), "0xc0");
  assert.equal(toHex(rlpList([rlpBytes(utf8("cat")), rlpBytes(utf8("dog"))])), "0xc88363617483646f67");
  // the set-theoretic representation of three
  const three: RlpItem = [[], [[]], [[], [[]]]];
  assert.equal(toHex(rlpEncode(three)), "0xc7c0c1c0c3c0c1c0");
  assert.deepEqual(rlpDecode(fromHex("0xc7c0c1c0c3c0c1c0")), three);
});

test("long payloads use a length-of-length header and round-trip", () => {
  for (const n of [55, 56, 255, 256, 70_000]) {
    const data = new Uint8Array(n).fill(0xab);
    const encoded = rlpBytes(data);
    assert.deepEqual(rlpDecode(encoded), data);
    const list = rlpList([encoded, encoded]);
    assert.equal(rlpItems(list).length, 2);
    assert.deepEqual(rlpDecode(list), [data, data]);
  }
});

test("non-canonical and truncated encodings are rejected", () => {
  assert.throws(() => rlpDecode(fromHex("0x8105"))); // single byte < 0x80 must be its own encoding
  assert.throws(() => rlpDecode(fromHex("0xb80100"))); // long form for a short string
  assert.throws(() => rlpDecode(fromHex("0x83646f"))); // truncated
  assert.throws(() => rlpDecode(fromHex("0x8000"))); // trailing bytes
});
