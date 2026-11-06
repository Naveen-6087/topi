// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

import { Test } from "forge-std/Test.sol";
import { ShieldedPool } from "../src/ShieldedPool.sol";
import { IShieldedPool } from "../src/interfaces/IShieldedPool.sol";
import { ILeanSphincsVerifier } from "../src/interfaces/ILeanSphincsVerifier.sol";
import { Text } from "./utils/Text.sol";

/// @notice The interfaces match the implementation and the selectors the Yul accounts hard-code.
contract InterfacesTest is Test {
    using Text for string;

    ShieldedPool pool; // never deployed: only its member selectors (incl. public getters) are read

    /// Lower-case "0x" + 8 hex digits, as the selectors are written in the Yul sources.
    function hex4(bytes4 selector) internal pure returns (string memory) {
        return vm.toString(abi.encodePacked(selector));
    }

    function testShieldedPoolFunctionSelectors() public view {
        assertEq(IShieldedPool.initialize.selector, pool.initialize.selector);
        assertEq(IShieldedPool.deposit.selector, pool.deposit.selector);
        assertEq(IShieldedPool.settle.selector, pool.settle.selector);
        assertEq(IShieldedPool.testbedSeedRoots.selector, pool.testbedSeedRoots.selector);
        assertEq(IShieldedPool.terminal.selector, pool.terminal.selector);
        assertEq(IShieldedPool.operator.selector, pool.operator.selector);
        assertEq(IShieldedPool.testbed.selector, pool.testbed.selector);
        assertEq(IShieldedPool.poolId.selector, pool.poolId.selector);
        assertEq(IShieldedPool.scope.selector, pool.scope.selector);
        assertEq(IShieldedPool.initialized.selector, pool.initialized.selector);
        assertEq(IShieldedPool.knownPoolRoot.selector, pool.knownPoolRoot.selector);
        assertEq(IShieldedPool.knownAssociationRoot.selector, pool.knownAssociationRoot.selector);
        assertEq(IShieldedPool.spent.selector, pool.spent.selector);
        assertEq(IShieldedPool.deposits.selector, pool.deposits.selector);
        assertEq(IShieldedPool.poolRoot.selector, pool.poolRoot.selector);
        assertEq(IShieldedPool.associationRoot.selector, pool.associationRoot.selector);
        assertEq(IShieldedPool.nextPoolIndex.selector, pool.nextPoolIndex.selector);
    }

    function testShieldedPoolEventsAndErrors() public pure {
        assertEq(IShieldedPool.Deposited.selector, ShieldedPool.Deposited.selector);
        assertEq(IShieldedPool.Settled.selector, ShieldedPool.Settled.selector);
        assertEq(IShieldedPool.AlreadyInitialized.selector, ShieldedPool.AlreadyInitialized.selector);
        assertEq(IShieldedPool.BadAmount.selector, ShieldedPool.BadAmount.selector);
        assertEq(IShieldedPool.NonCanonical.selector, ShieldedPool.NonCanonical.selector);
        assertEq(IShieldedPool.NotPool.selector, ShieldedPool.NotPool.selector);
        assertEq(IShieldedPool.WrongSelector.selector, ShieldedPool.WrongSelector.selector);
        assertEq(IShieldedPool.ProofRejected.selector, ShieldedPool.ProofRejected.selector);
        assertEq(IShieldedPool.RootMismatch.selector, ShieldedPool.RootMismatch.selector);
        assertEq(IShieldedPool.WrongPool.selector, ShieldedPool.WrongPool.selector);
        assertEq(IShieldedPool.UnknownRoot.selector, ShieldedPool.UnknownRoot.selector);
        assertEq(IShieldedPool.AlreadySpent.selector, ShieldedPool.AlreadySpent.selector);
        assertEq(IShieldedPool.NotTestbed.selector, ShieldedPool.NotTestbed.selector);
        assertEq(IShieldedPool.BadContext.selector, ShieldedPool.BadContext.selector);
        assertEq(IShieldedPool.PayoutFailed.selector, ShieldedPool.PayoutFailed.selector);
    }

    /// PoolAccount.yul approves only a SENDER frame calling settle, and its initcode calls initialize().
    function testPoolAccountSelectors() public view {
        string memory yul = vm.readFile("yul/PoolAccount.yul");
        assertEq(IShieldedPool.settle.selector, bytes4(0xf4b589c7));
        assertTrue(yul.contains(string.concat("s := ", hex4(IShieldedPool.settle.selector))));
        assertTrue(yul.contains(string.concat("shl(224, ", hex4(IShieldedPool.initialize.selector), ")")));
    }

    /// PQPaymaster.yul calls verify(bytes32,bytes32,bytes32,bytes) on the leanSPHINCS verifier.
    function testLeanSphincsVerifierSelector() public view {
        assertEq(ILeanSphincsVerifier.verify.selector, bytes4(0x2bf6eda8));
        string memory yul = vm.readFile("yul/PQPaymaster.yul");
        assertTrue(yul.contains(string.concat("shl(224, ", hex4(ILeanSphincsVerifier.verify.selector), ")")));
    }

    /// Both paymasters accept withdraw() from their admin/operator.
    function testPaymasterWithdrawSelector() public view {
        string memory sel = hex4(bytes4(keccak256("withdraw()")));
        assertEq(sel, "0x3ccfd60b");
        assertTrue(vm.readFile("yul/PQPaymaster.yul").contains(string.concat("eq(shr(224, calldataload(0)), ", sel, ")")));
        assertTrue(
            vm.readFile("yul/ClassicPaymaster.yul").contains(string.concat("eq(shr(224, calldataload(0)), ", sel, ")"))
        );
    }
}
