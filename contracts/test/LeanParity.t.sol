// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

import { Test } from "forge-std/Test.sol";
import { IShieldedPool } from "../src/interfaces/IShieldedPool.sol";
import { Text } from "./utils/Text.sol";

/// @notice The constants the Yul accounts hard-code appear with the same values (and the same failure
/// codes) in the Lean model of the frame policy, lean/PoolModel/Frames.lean. Each check builds the Yul
/// and the Lean snippet from one Solidity constant, so a change on either side fails here.
contract LeanParityTest is Test {
    using Text for string;

    string constant LEAN_PATH = "../lean/PoolModel/Frames.lean";

    uint256 constant MODE_VERIFY = 1;
    uint256 constant MODE_SENDER = 2;
    uint256 constant SCOPE_PAYMENT = 1;
    uint256 constant SCOPE_EXECUTION = 2;
    uint256 constant POOL_FRAME_COUNT = 3;
    uint256 constant SCHEME_ARBITRARY = 0;
    uint256 constant SCHEME_SECP256K1 = 1;
    uint256 constant LEANSPHINCS_SIG_BYTES = 5684;
    uint256 constant SELECTOR_BYTES = 4;

    string lean;
    string poolAccount;
    string pqPaymaster;
    string classicPaymaster;

    function setUp() public {
        lean = vm.readFile(LEAN_PATH);
        poolAccount = vm.readFile("yul/PoolAccount.yul");
        pqPaymaster = vm.readFile("yul/PQPaymaster.yul");
        classicPaymaster = vm.readFile("yul/ClassicPaymaster.yul");
    }

    function s(uint256 v) internal pure returns (string memory) {
        return vm.toString(v);
    }

    function has(string memory source, string memory name, string memory needle) internal pure {
        assertTrue(source.contains(needle), string.concat(name, " lacks: ", needle));
    }

    function testSettleSelector() public view {
        string memory sel = vm.toString(abi.encodePacked(IShieldedPool.settle.selector));
        has(lean, "Frames.lean", string.concat("def SETTLE : Nat := ", sel));
        has(poolAccount, "PoolAccount.yul", string.concat("s := ", sel));
        // Frame 2 must carry at least a selector, and that selector must be settle's.
        has(poolAccount, "PoolAccount.yul", string.concat("if lt(frameParam(2, 0x04), ", s(SELECTOR_BYTES), ") { fail(34) }"));
        has(lean, "Frames.lean", string.concat("if f2.dataLen < ", s(SELECTOR_BYTES), " then .error 34"));
        has(poolAccount, "PoolAccount.yul", "settleSelector())) { fail(35) }");
        has(lean, "Frames.lean", unicode"if f2.selector ≠ SETTLE then .error 35");
    }

    function testFrameModes() public view {
        has(lean, "Frames.lean", string.concat("| .verify => ", s(MODE_VERIFY)));
        has(lean, "Frames.lean", string.concat("| .sender => ", s(MODE_SENDER)));
        // Pool: frames 0 and 1 are VERIFY, frame 2 is SENDER.
        has(poolAccount, "PoolAccount.yul", string.concat("eq(frameParam(0, 0x02), ", s(MODE_VERIFY), ")) { fail(11) }"));
        has(lean, "Frames.lean", unicode"if f0.mode ≠ .verify then .error 11");
        has(poolAccount, "PoolAccount.yul", string.concat("eq(frameParam(1, 0x02), ", s(MODE_VERIFY), ")) { fail(21) }"));
        has(lean, "Frames.lean", unicode"if f1.mode ≠ .verify then .error 21");
        has(poolAccount, "PoolAccount.yul", string.concat("eq(frameParam(2, 0x02), ", s(MODE_SENDER), ")) { fail(31) }"));
        has(lean, "Frames.lean", unicode"if f2.mode ≠ .sender then .error 31");
        // Paymasters: their own frame (1) is VERIFY.
        string memory pm = string.concat("eq(frameParam(1, 0x02), ", s(MODE_VERIFY), ")) { fail(4) }");
        has(pqPaymaster, "PQPaymaster.yul", pm);
        has(classicPaymaster, "ClassicPaymaster.yul", pm);
        has(lean, "Frames.lean", unicode"if f1.mode ≠ .verify then .error 4");
    }

    function testApproveScopesAndFrameFlags() public view {
        has(lean, "Frames.lean", string.concat("def SCOPE_PAYMENT : Nat := ", s(SCOPE_PAYMENT)));
        has(lean, "Frames.lean", string.concat("def SCOPE_EXECUTION : Nat := ", s(SCOPE_EXECUTION)));
        // APPROVE scopes.
        has(poolAccount, "PoolAccount.yul", string.concat("verbatim_3i_0o(hex\"AA\", 0, 0, ", s(SCOPE_EXECUTION), ")"));
        has(classicPaymaster, "ClassicPaymaster.yul", string.concat("verbatim_3i_0o(hex\"AA\", 0, 0, ", s(SCOPE_PAYMENT), ")"));
        has(pqPaymaster, "PQPaymaster.yul", string.concat("approve(", s(SCOPE_PAYMENT), ")"));
        // Frame flags: pool frame 0 = execution, payer frame 1 = payment, settle frame 2 = none.
        has(poolAccount, "PoolAccount.yul", string.concat("eq(frameParam(0, 0x03), ", s(SCOPE_EXECUTION), ")) { fail(12) }"));
        has(lean, "Frames.lean", unicode"if f0.flags ≠ SCOPE_EXECUTION then .error 12");
        has(poolAccount, "PoolAccount.yul", string.concat("eq(frameParam(1, 0x03), ", s(SCOPE_PAYMENT), ")) { fail(22) }"));
        has(lean, "Frames.lean", unicode"if f1.flags ≠ SCOPE_PAYMENT then .error 22");
        has(poolAccount, "PoolAccount.yul", "if frameParam(2, 0x03) { fail(32) }");
        has(lean, "Frames.lean", unicode"if f2.flags ≠ 0 then .error 32");
        string memory pm = string.concat("eq(frameParam(1, 0x03), ", s(SCOPE_PAYMENT), ")) { fail(5) }");
        has(pqPaymaster, "PQPaymaster.yul", pm);
        has(classicPaymaster, "ClassicPaymaster.yul", pm);
        has(lean, "Frames.lean", unicode"if f1.flags ≠ SCOPE_PAYMENT then .error 5");
    }

    function testFrameLayout() public view {
        // Pool transactions have exactly three frames and the pool's VERIFY entry runs as frame 0.
        has(poolAccount, "PoolAccount.yul", string.concat("eq(txParam(0x09), ", s(POOL_FRAME_COUNT), ")) { fail(2) }"));
        has(lean, "Frames.lean", string.concat(unicode"if tx.frames.length ≠ ", s(POOL_FRAME_COUNT), " then .error 2"));
        has(poolAccount, "PoolAccount.yul", "if txParam(0x0A) { fail(3) }");
        has(lean, "Frames.lean", unicode"if current ≠ 0 then .error 3");
        // The paymaster's VERIFY entry runs as frame 1 and frame 1 targets the paymaster.
        string memory current = "eq(txParam(0x0A), 1)) { fail(2) }";
        has(pqPaymaster, "PQPaymaster.yul", current);
        has(classicPaymaster, "ClassicPaymaster.yul", current);
        has(lean, "Frames.lean", unicode"if current ≠ 1 then .error 2");
        string memory target = "eq(frameParam(1, 0x00), address())) { fail(3) }";
        has(pqPaymaster, "PQPaymaster.yul", target);
        has(classicPaymaster, "ClassicPaymaster.yul", target);
        has(lean, "Frames.lean", unicode"if f1.target ≠ paymaster then .error 3");
    }

    function testLeanSphincsSignatureEntry() public view {
        has(lean, "Frames.lean", string.concat("def ARBITRARY : Nat := ", s(SCHEME_ARBITRARY)));
        has(lean, "Frames.lean", string.concat("def LEANSPHINCS_SIG_BYTES : Nat := ", s(LEANSPHINCS_SIG_BYTES)));
        // ARBITRARY is scheme 0, so the Yul tests the scheme for non-zero.
        assertEq(SCHEME_ARBITRARY, 0);
        has(pqPaymaster, "PQPaymaster.yul", "if sigScheme(0) { fail(7) }");
        has(lean, "Frames.lean", unicode"if s.scheme ≠ ARBITRARY then .error 7");
        has(pqPaymaster, "PQPaymaster.yul", "if sigMsg(0) { fail(8) }");
        has(lean, "Frames.lean", "if !s.msgEmpty then .error 8");
        has(pqPaymaster, "PQPaymaster.yul", string.concat("if iszero(eq(len, ", s(LEANSPHINCS_SIG_BYTES), ")) { fail(9) }"));
        has(lean, "Frames.lean", unicode"if s.len ≠ LEANSPHINCS_SIG_BYTES then .error 9");
        has(pqPaymaster, "PQPaymaster.yul", "if iszero(eq(mload(0x00), 1)) { fail(11) }");
        has(lean, "Frames.lean", "if !s.verifies then .error 11");
    }

    function testClassicSignatureEntry() public view {
        has(classicPaymaster, "ClassicPaymaster.yul", string.concat("eq(sigParam(0, 1), ", s(SCHEME_SECP256K1), ")) { fail(8) }"));
        has(lean, "Frames.lean", string.concat(unicode"if s.scheme ≠ ", s(SCHEME_SECP256K1), " then .error 8"));
        has(classicPaymaster, "ClassicPaymaster.yul", "if iszero(eq(sigParam(0, 0), operator)) { fail(7) }");
        has(lean, "Frames.lean", unicode"if s.signer ≠ operator then .error 7");
        has(classicPaymaster, "ClassicPaymaster.yul", "if sigParam(0, 2) { fail(9) }");
        has(lean, "Frames.lean", "if !s.msgEmpty then .error 9");
    }
}
