// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

import { Test, console2 } from "forge-std/Test.sol";
import { ShieldedPool, PackedTree } from "../src/ShieldedPool.sol";
import { LeanVmGroupedTerminal_KeccakPow28T6Lean as Terminal } from
    "../src/verifier/LeanVmGroupedTerminal_KeccakPow28T6Lean.sol";

/// @notice ShieldedPool called directly, settling the real terminal proof fixture. In production the pool
/// runs by DELEGATECALL from the pool account, where `address(this)` is the pool; here it is the logic
/// contract itself, so "the pool" is the logic's address.
contract ShieldedPoolTest is Test {
    string json;
    bytes proofCall;
    uint256[] a;
    uint256[] b;

    function setUp() public {
        json = vm.readFile("test/fixtures/lean/lean-vectors.json");
        proofCall = vm.readFileBinary("test/fixtures/terminal/calldata.bin");
        a = vm.parseJsonUintArray(json, ".fields[0]");
        b = vm.parseJsonUintArray(json, ".fields[1]");
    }

    function eight(uint256[] memory f, uint256 start) internal pure returns (uint256[8] memory d) {
        for (uint256 i = 0; i < 8; ++i) d[i] = f[start + i];
    }

    function packed(uint256[] memory f, uint256 start) internal pure returns (uint256) {
        return PackedTree.pack(eight(f, start));
    }

    function pool(bool testbed) internal returns (ShieldedPool p) {
        p = new ShieldedPool(address(new Terminal()), address(this), testbed, eight(a, 0), eight(a, 8));
        p.initialize();
        vm.deal(address(p), 1 ether);
    }

    receive() external payable { }

    function testInitializeOnce() public {
        ShieldedPool p = pool(true);
        vm.expectRevert(ShieldedPool.AlreadyInitialized.selector);
        p.initialize();
        assertTrue(p.knownPoolRoot(p.poolRoot()));
        assertTrue(p.knownAssociationRoot(p.associationRoot()));
    }

    function testDeposits() public {
        ShieldedPool p = pool(true);
        uint256[8] memory pre;
        pre[0] = 42;
        uint256 r0 = p.poolRoot();
        uint256 g = gasleft();
        p.deposit{ value: 0.01 ether }(pre);
        console2.log("deposit gas (first)", g - gasleft());
        pre[0] = 43;
        g = gasleft();
        p.deposit{ value: 0.02 ether }(pre);
        console2.log("deposit gas (second)", g - gasleft());
        assertEq(p.nextPoolIndex(), 4); // each deposit = note + zero leaf
        assertEq(p.deposits(), 2);
        assertTrue(p.poolRoot() != r0);
        assertTrue(p.knownPoolRoot(p.poolRoot()) && p.knownAssociationRoot(p.associationRoot()));
        vm.expectRevert(ShieldedPool.BadAmount.selector);
        p.deposit{ value: 0 }(pre);
        pre[1] = 2_130_706_433;
        vm.expectRevert(ShieldedPool.NonCanonical.selector);
        p.deposit{ value: 1 }(pre);
    }

    function seed(ShieldedPool p) internal {
        p.testbedSeedRoots(packed(a, 50), packed(a, 58));
        p.testbedSeedRoots(packed(b, 50), packed(b, 58));
    }

    function testSettleTestbed() public {
        ShieldedPool p = pool(true);
        uint256[8] memory pre;
        p.deposit{ value: 0.01 ether }(pre); // keeps the tree mid-stream (index 2 after)
        seed(p);
        uint256 before = p.nextPoolIndex();
        vm.prank(address(p));
        uint256 g = gasleft();
        p.settle(proofCall, a, b);
        console2.log("settle gas (in-call, test-compiled terminal)", g - gasleft());
        assertEq(p.nextPoolIndex(), before + 2);
        assertTrue(p.spent(packed(a, 66)) && p.spent(packed(b, 66)));
        assertTrue(p.knownPoolRoot(p.poolRoot()));
        vm.prank(address(p));
        vm.expectRevert(ShieldedPool.AlreadySpent.selector);
        p.settle(proofCall, a, b);
    }

    function testSettleOnlyByPool() public {
        ShieldedPool p = pool(true);
        seed(p);
        vm.expectRevert(ShieldedPool.NotPool.selector);
        p.settle(proofCall, a, b);
    }

    function testSettleNeedsKnownRoots() public {
        ShieldedPool p = pool(true);
        vm.prank(address(p));
        vm.expectRevert(ShieldedPool.UnknownRoot.selector);
        p.settle(proofCall, a, b);
    }

    function testSettleRejectsTamperedFields() public {
        ShieldedPool p = pool(true);
        seed(p);
        uint256[] memory changed = a;
        changed[35] += 1; // withdrawn amount limb
        vm.prank(address(p));
        vm.expectRevert(ShieldedPool.RootMismatch.selector);
        p.settle(proofCall, changed, b);
    }

    function testSettleRejectsAlteredRecipient() public {
        ShieldedPool p = pool(true);
        seed(p);
        uint256[] memory changed = a;
        changed[24] += 1; // first recipient limb
        vm.prank(address(p));
        vm.expectRevert(ShieldedPool.RootMismatch.selector);
        p.settle(proofCall, changed, b);
    }

    function testNonTestbedHasNoSeeding() public {
        ShieldedPool p = pool(false);
        vm.expectRevert(ShieldedPool.NotTestbed.selector);
        p.testbedSeedRoots(1, 1);
    }
}
