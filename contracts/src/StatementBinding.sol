// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

import { LeanVmPoseidon1 } from "sol-whir-p3/leanvm/LeanVmPoseidon1.sol";

/// @title StatementBinding
/// @notice Recomputes what the leanVM terminal proof commits to, so the pool can bind a verified proof
/// to its own spends' public inputs (the terminal verifier itself only returns bool).
/// @dev statementDigest: spartan-whir `full_zk_statement_digest` with the Poseidon1 engine
///   ("poseidon1-width-16-length-prefixed-statement-v1"): a Plonky3 duplex challenger (width 16,
///   rate 8) observing [32, "leanvm-spartan-whir-statement-v1", n, inputs...], then finalize.
/// applicationRoot: leanVM `binary_application_root` = poseidon_hash_slice([NOD1, 2, 0 x6, A, B]).
library StatementBinding {
    uint256 internal constant P = 2_130_706_433;
    uint256 internal constant NOD1 = 1_313_817_649;
    bytes internal constant DOMAIN = "leanvm-spartan-whir-statement-v1";
    error NonCanonicalField();

    function statementDigest(uint256[] memory inputs) internal pure returns (uint256[8] memory digest) {
        uint256 n = 2 + DOMAIN.length + inputs.length;
        uint256[] memory pre = new uint256[](n);
        pre[0] = DOMAIN.length;
        for (uint256 i = 0; i < DOMAIN.length; ++i) pre[1 + i] = uint8(DOMAIN[i]);
        pre[1 + DOMAIN.length] = inputs.length;
        for (uint256 i = 0; i < inputs.length; ++i) {
            if (inputs[i] >= P) revert NonCanonicalField();
            pre[2 + DOMAIN.length + i] = inputs[i];
        }

        uint256[16] memory s;
        uint256 used;
        for (uint256 i = 0; i < n; ++i) {
            s[used++] = pre[i];
            if (used == 8) {
                // Full rate block: overwrite rate, tag the absorbed length in the first capacity slot.
                s[8] = addmod(s[8], 8, P);
                LeanVmPoseidon1.permute(s);
                used = 0;
            }
        }
        // finalize(): one more duplexing; a partial block zero-fills the rate and is length-tagged.
        if (used > 0) {
            for (uint256 i = used; i < 8; ++i) s[i] = 0;
            s[8] = addmod(s[8], used, P);
        }
        LeanVmPoseidon1.permute(s);
        for (uint256 i = 0; i < 8; ++i) digest[i] = s[i];
    }

    /// Lean-statement variant (public input = the 8-element transaction digest). The first 32 preimage
    /// elements [32, DOMAIN[0..31]] are constant, so the duplex state after those four rate blocks is
    /// precomputed (printed by test/Codegen.t.sol); only two permutations remain.
    function leanStatementDigest(uint256[8] memory txDigest) internal pure returns (uint256[8] memory digest) {
        uint256[16] memory s = [
            uint256(1803224574), 1468768875, 745590971, 300623592, 1375899064, 38580362, 1985670189, 760799331,
            210462114, 1996202588, 791428287, 121267827, 775329656, 2079012177, 1496001616, 2071744024
        ];
        for (uint256 i = 0; i < 8; ++i) {
            if (txDigest[i] >= P) revert NonCanonicalField();
        }
        // Block 5: DOMAIN[31] = '1', the input count 8, inputs 0..5.
        s[0] = 49;
        s[1] = 8;
        for (uint256 i = 0; i < 6; ++i) s[2 + i] = txDigest[i];
        s[8] = addmod(s[8], 8, P);
        LeanVmPoseidon1.permute(s);
        // finalize(): inputs 6..7, zero-filled rate, length tag 2.
        s[0] = txDigest[6];
        s[1] = txDigest[7];
        for (uint256 i = 2; i < 8; ++i) s[i] = 0;
        s[8] = addmod(s[8], 2, P);
        LeanVmPoseidon1.permute(s);
        for (uint256 i = 0; i < 8; ++i) digest[i] = s[i];
    }

    function applicationRoot(uint256[8] memory left, uint256[8] memory right)
        internal
        pure
        returns (uint256[8] memory)
    {
        uint256[] memory pre = new uint256[](24);
        pre[0] = NOD1;
        pre[1] = 2;
        for (uint256 i = 0; i < 8; ++i) {
            pre[8 + i] = left[i];
            pre[16 + i] = right[i];
        }
        return LeanVmPoseidon1.hashWords(pre);
    }
}
