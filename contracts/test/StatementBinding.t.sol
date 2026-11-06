// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

import { Test, console2 } from "forge-std/Test.sol";
import { LeanVmPoseidon1 } from "sol-whir-p3/leanvm/LeanVmPoseidon1.sol";
import { KoalaPoseidon2x24 } from "../src/hash/KoalaPoseidon2x24.sol";
import { StatementBinding } from "../src/StatementBinding.sol";

/// @notice Checks the Solidity hashing against vectors printed by Rust (codegen/vectors.rs), which
/// asserted the statement digests against the fixtures behind the proof verified on chain 8141.
contract StatementBindingTest is Test {
    string json;

    function setUp() public {
        json = vm.readFile("test/fixtures/vectors.json");
    }

    function arr(string memory key) internal view returns (uint256[] memory) {
        return vm.parseJsonUintArray(json, key);
    }

    function eight(uint256[] memory a) internal pure returns (uint256[8] memory out) {
        require(a.length == 8, "len");
        for (uint256 i = 0; i < 8; ++i) out[i] = a[i];
    }

    function testPoseidon1Permutation() public view {
        uint256[16] memory s;
        for (uint256 i = 0; i < 16; ++i) s[i] = i;
        LeanVmPoseidon1.permute(s);
        uint256[] memory want = arr(".poseidon1_16_permutation.output");
        for (uint256 i = 0; i < 16; ++i) assertEq(s[i], want[i]);
    }

    function testPoseidon2HashFields() public view {
        uint256[] memory input = arr(".poseidon2_24_hash_fields.input");
        uint256[8] memory got = KoalaPoseidon2x24.hashFields(4, input);
        uint256[] memory want = arr(".poseidon2_24_hash_fields.output");
        for (uint256 i = 0; i < 8; ++i) assertEq(got[i], want[i]);
        uint256[8] memory l;
        uint256[8] memory r;
        for (uint256 i = 0; i < 8; ++i) (l[i], r[i]) = (input[i], input[8 + i]);
        uint256[8] memory viaNode = KoalaPoseidon2x24.node(l, r);
        for (uint256 i = 0; i < 8; ++i) assertEq(viaNode[i], want[i]);
    }

    function testStatementDigestsAndApplicationRoot() public view {
        uint256 g0 = gasleft();
        uint256[8] memory a = StatementBinding.statementDigest(arr(".statements[0].public_inputs"));
        uint256 g1 = gasleft();
        uint256[8] memory b = StatementBinding.statementDigest(arr(".statements[1].public_inputs"));
        uint256 g2 = gasleft();
        uint256[8] memory root = StatementBinding.applicationRoot(a, b);
        uint256 g3 = gasleft();
        assertEq(abi.encode(a), abi.encode(eight(arr(".statements[0].digest"))));
        assertEq(abi.encode(b), abi.encode(eight(arr(".statements[1].digest"))));
        assertEq(abi.encode(root), abi.encode(eight(arr(".application_root"))));
        console2.log("statement digest gas", g0 - g1, g1 - g2);
        console2.log("application root gas", g2 - g3);
    }

    function testSwappedOrChangedStatementsGiveAnotherRoot() public view {
        uint256[8] memory a = eight(arr(".statements[0].digest"));
        uint256[8] memory b = eight(arr(".statements[1].digest"));
        uint256[8] memory want = eight(arr(".application_root"));
        assertTrue(keccak256(abi.encode(StatementBinding.applicationRoot(b, a))) != keccak256(abi.encode(want)));
        uint256[] memory inputs = arr(".statements[0].public_inputs");
        inputs[24] += 1; // first recipient limb
        uint256[8] memory changed = StatementBinding.statementDigest(inputs);
        assertTrue(keccak256(abi.encode(StatementBinding.applicationRoot(changed, b))) != keccak256(abi.encode(want)));
    }
}

/// @notice Lean-statement binding (the one ShieldedPool uses) against the lean fixtures behind the
/// terminal proof in test/fixtures/terminal.
contract LeanStatementBindingTest is Test {
    string json;

    function setUp() public {
        json = vm.readFile("test/fixtures/lean/lean-vectors.json");
    }

    function arr(string memory key) internal view returns (uint256[] memory) {
        return vm.parseJsonUintArray(json, key);
    }

    function eight(uint256[] memory a) internal pure returns (uint256[8] memory out) {
        for (uint256 i = 0; i < 8; ++i) out[i] = a[i];
    }

    function testLeanBinding() public view {
        uint256[8][2] memory d;
        for (uint256 k = 0; k < 2; ++k) {
            string memory i = k == 0 ? "[0]" : "[1]";
            uint256 g0 = gasleft();
            uint256[8] memory tx_ = KoalaPoseidon2x24.hashFields(6, arr(string.concat(".fields", i)));
            uint256 g1 = gasleft();
            d[k] = StatementBinding.leanStatementDigest(tx_);
            uint256 g2 = gasleft();
            assertEq(abi.encode(tx_), abi.encode(eight(arr(string.concat(".tx_digest", i)))));
            assertEq(abi.encode(d[k]), abi.encode(eight(arr(string.concat(".statement_digest", i)))));
            console2.log("tx digest gas", g0 - g1, "lean statement digest gas", g1 - g2);
        }
        uint256 g3 = gasleft();
        uint256[8] memory root = StatementBinding.applicationRoot(d[0], d[1]);
        console2.log("application root gas", g3 - gasleft());
        assertEq(abi.encode(root), abi.encode(eight(arr(".application_root"))));
    }

    /// The precomputed lean digest equals the generic statement digest over the same 8 inputs.
    function testLeanDigestMatchesGenericDigest() public pure {
        uint256[8] memory tx_ = [uint256(1), 2, 3, 4, 5, 6, 7, 2_130_706_432];
        uint256[] memory inputs = new uint256[](8);
        for (uint256 i = 0; i < 8; ++i) inputs[i] = tx_[i];
        assertEq(
            abi.encode(StatementBinding.leanStatementDigest(tx_)), abi.encode(StatementBinding.statementDigest(inputs))
        );
    }
}

/// @notice Gas of one permutation of each hash (printed, not asserted).
contract PermutationGasTest is Test {
    function testSinglePermutationGas() public view {
        uint256[16] memory s1;
        uint256[24] memory s2;
        for (uint256 i = 0; i < 16; ++i) s1[i] = i;
        for (uint256 i = 0; i < 24; ++i) s2[i] = i;
        uint256 g0 = gasleft();
        LeanVmPoseidon1.permute(s1);
        uint256 g1 = gasleft();
        KoalaPoseidon2x24.permute(s2);
        uint256 g2 = gasleft();
        console2.log("poseidon1-16 permute gas", g0 - g1);
        console2.log("poseidon2-24 permute gas", g1 - g2);
    }
}
