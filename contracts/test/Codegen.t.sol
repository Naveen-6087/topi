// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

import { Test, console2 } from "forge-std/Test.sol";
import { LeanVmPoseidon1 } from "sol-whir-p3/leanvm/LeanVmPoseidon1.sol";
import { KoalaPoseidon2x24 } from "../src/hash/KoalaPoseidon2x24.sol";

// Printers for the constants baked into the sources. Run with -vv:
//   forge test --match-contract StatementPrefixState -vv   (StatementBinding.leanStatementDigest's initial state)
//   forge test --match-contract PoolZeroHashes -vv         (codegen/zeros.txt)
//   forge test --match-contract AssociationZeroHashes -vv  (codegen/zeros-association.txt)

/// @notice The Poseidon1 duplex state after absorbing the constant statement prefix
/// [32, "leanvm-spartan-whir-statement-v1"[0..31]] (four full rate blocks).
contract StatementPrefixState is Test {
    function testPrintPrefixState() public pure {
        bytes memory d = "leanvm-spartan-whir-statement-v1";
        uint256[16] memory s;
        uint256[32] memory pre;
        pre[0] = 32;
        for (uint256 i = 0; i < 31; ++i) pre[1 + i] = uint8(d[i]);
        for (uint256 blk = 0; blk < 4; ++blk) {
            for (uint256 i = 0; i < 8; ++i) s[i] = pre[8 * blk + i];
            s[8] = addmod(s[8], 8, 2_130_706_433);
            LeanVmPoseidon1.permute(s);
        }
        for (uint256 i = 0; i < 16; ++i) console2.log(s[i]);
    }
}

/// @notice The packed empty-subtree roots of the depth-32 pool tree (node domain 4, zero leaf).
contract PoolZeroHashes is Test {
    function testPrintZeros() public pure {
        uint256[8] memory z;
        for (uint256 i = 0; i <= 32; ++i) {
            uint256 w;
            for (uint256 j = 0; j < 8; ++j) w |= z[j] << (32 * j);
            console2.logBytes32(bytes32(w));
            z = KoalaPoseidon2x24.node(z, z);
        }
    }
}

/// @notice The packed empty-subtree roots of the depth-32 association tree (node domain 5, zero leaf).
contract AssociationZeroHashes is Test {
    function testPrintZerosAssociation() public pure {
        uint256[8] memory z;
        for (uint256 i = 0; i <= 32; ++i) {
            uint256 w;
            for (uint256 j = 0; j < 8; ++j) w |= z[j] << (32 * j);
            console2.logBytes32(bytes32(w));
            z = KoalaPoseidon2x24.nodeIn(5, z, z);
        }
    }
}
