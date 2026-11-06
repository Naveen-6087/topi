// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

import { Test, console2 } from "forge-std/Test.sol";
import { ITerminalVerifier } from "../src/interfaces/ITerminalVerifier.sol";
import { LeanVmGroupedTerminal_KeccakPow28T6Lean as Terminal } from
    "../src/verifier/LeanVmGroupedTerminal_KeccakPow28T6Lean.sol";

/// @notice The generated terminal verifier against the real proof fixture (test/fixtures/terminal),
/// and the calldata layout ShieldedPool relies on.
contract TerminalVerifierTest is Test {
    address terminal;
    bytes proofCall;

    function setUp() public {
        terminal = address(new Terminal());
        proofCall = vm.readFileBinary("test/fixtures/terminal/calldata.bin");
    }

    function testSelector() public view {
        assertEq(ITerminalVerifier.verifyGroupedLogupV1.selector, bytes4(0x5801ce09));
        assertEq(bytes4(proofCall), ITerminalVerifier.verifyGroupedLogupV1.selector);
        string memory manifest = vm.readFile("test/fixtures/terminal/manifest.json");
        assertEq(vm.parseJsonString(manifest, ".verification_selector"), "5801ce09");
        assertEq(vm.parseJsonUint(manifest, ".calldata_bytes"), proofCall.length);
    }

    function testAcceptsFixture() public view {
        uint256 g = gasleft();
        (bool ok, bytes memory ret) = terminal.staticcall(proofCall);
        console2.log("terminal verify gas (in-call)", g - gasleft());
        assertTrue(ok);
        assertEq(ret.length, 32);
        assertEq(abi.decode(ret, (uint256)), 1);
    }

    /// ShieldedPool reads proof.applicationRoot at calldata offset 100.
    function testApplicationRootOffset() public view {
        (ITerminalVerifier.Proof memory proof) = abi.decode(_args(proofCall), (ITerminalVerifier.Proof));
        for (uint256 i = 0; i < 8; ++i) {
            bytes memory word = new bytes(32);
            for (uint256 k = 0; k < 32; ++k) word[k] = proofCall[100 + 32 * i + k];
            assertEq(uint256(bytes32(word)), proof.applicationRoot[i]);
        }
    }

    function testRejectsChangedApplicationRoot() public {
        bytes memory changed = proofCall;
        changed[100 + 31] = bytes1(uint8(changed[100 + 31]) ^ 1);
        (bool ok, bytes memory ret) = terminal.call(changed);
        assertTrue(!ok || abi.decode(ret, (uint256)) != 1);
    }

    function _args(bytes memory call_) internal pure returns (bytes memory args) {
        args = new bytes(call_.length - 4);
        for (uint256 i = 0; i < args.length; ++i) args[i] = call_[4 + i];
    }
}
