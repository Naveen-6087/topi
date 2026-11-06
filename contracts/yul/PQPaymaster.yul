// SPDX-License-Identifier: MIT OR Apache-2.0
/// @title PQPaymaster
/// @notice Pays gas for shielded pool transactions that the exit operator signed with leanSPHINCS
/// (hash-based, post-quantum). A call whose calldata is exactly 4 zero bytes is the VERIFY entry: it
/// approves PAYMENT (scope 1) only for this pool's transactions carrying exactly one signature entry:
/// an ARBITRARY entry (scheme 0) with empty msg, i.e. a witness over compute_sig_hash(tx), that the shared
/// leanSPHINCS verifier accepts under the operator's public key. EIP-8141 elides that entry's bytes from
/// the signature hash, so the witness can sign the whole transaction, this frame included.
/// Plain transfers fund the paymaster; the admin can withdraw with calldata 0x3ccfd60b (withdraw()).
/// @dev Runtime layout: this code, then five 32-byte words: pool, admin, verifier, pkSeed, pkRoot.
/// The SIGPARAM / SIGDATACOPY / TXPARAM / APPROVE byte sequences follow nconsigny/SPHINCS-'s
/// SphincsFrameVerify.yul (MIT); the frame checks follow ClassicPaymaster.yul. The verifier implements
/// ILeanSphincsVerifier (src/interfaces). Modelled in lean/PoolModel/Frames.lean (paymasterPQPolicy).
object "PQPaymaster" {
    code {
        let rsize := datasize("runtime")
        datacopy(0, dataoffset("runtime"), rsize)
        codecopy(rsize, sub(codesize(), 160), 160)
        return(0, add(rsize, 160))
    }

    object "runtime" {
        code {
            function txParam(param) -> value { value := verbatim_1i_1o(hex"B0", param) }
            function frameParam(frameIndex, param) -> value {
                value := verbatim_2i_1o(hex"B3", frameIndex, param)
            }
            // SIGPARAM(param, i): [i] PUSH1 param SWAP1 SIGPARAM  (signature index on top)
            function sigScheme(i) -> v { v := verbatim_1i_1o(hex"600190b4", i) }
            function sigMsg(i) -> v { v := verbatim_1i_1o(hex"600290b4", i) }
            function sigLen(i) -> v { v := verbatim_1i_1o(hex"600390b4", i) }
            // SIGDATACOPY(memOffset 0xa4, dataOffset 0, length = SIGPARAM(0x03, i), i)
            function sigCopyToArgs(i) { verbatim_1i_0o(hex"80600390b460006100a4b5", i) }
            // APPROVE(0, 0, scope)
            function approve(scope) { verbatim_1i_0o(hex"60006000aa", scope) }

            function word(i) -> w {
                codecopy(0, add(sub(codesize(), 160), mul(i, 32)), 32)
                w := mload(0)
            }
            function fail(code) {
                mstore(0, code)
                revert(0, 32)
            }

            if and(eq(calldatasize(), 4), iszero(shr(224, calldataload(0)))) {
                let pool := word(0)
                let verifier := word(2)
                let pkSeed := word(3)
                let pkRoot := word(4)
                // Only the configured pool's transactions, with this paymaster as frame 1.
                if iszero(eq(txParam(0x02), pool)) { fail(1) }
                if iszero(eq(txParam(0x0A), 1)) { fail(2) }
                if iszero(eq(frameParam(1, 0x00), address())) { fail(3) }
                if iszero(eq(frameParam(1, 0x02), 1)) { fail(4) }
                if iszero(eq(frameParam(1, 0x03), 1)) { fail(5) }
                // Exactly one signature: ARBITRARY, empty msg, leanSPHINCS length.
                if iszero(eq(txParam(0x0B), 1)) { fail(6) }
                if sigScheme(0) { fail(7) }
                if sigMsg(0) { fail(8) }
                let len := sigLen(0)
                if iszero(eq(len, 5684)) { fail(9) }
                // verify(bytes32 pkSeed, bytes32 pkRoot, bytes32 message, bytes sig) on the shared verifier,
                // message = compute_sig_hash(tx) (TXPARAM 0x08).
                mstore(0x00, shl(224, 0x2bf6eda8))
                mstore(0x04, pkSeed)
                mstore(0x24, pkRoot)
                mstore(0x44, txParam(0x08))
                mstore(0x64, 0x80)
                mstore(0x84, len)
                sigCopyToArgs(0)
                let argsLen := add(0xa4, and(add(len, 31), not(31)))
                let ok := staticcall(gas(), verifier, 0x00, argsLen, 0x00, 0x20)
                if iszero(ok) { fail(10) }
                if iszero(eq(mload(0x00), 1)) { fail(11) }
                approve(1)
                stop()
            }
            if callvalue() { stop() }
            if and(eq(caller(), word(1)), eq(shr(224, calldataload(0)), 0x3ccfd60b)) {
                if iszero(call(gas(), word(1), selfbalance(), 0, 0, 0, 0)) { revert(0, 0) }
                stop()
            }
            revert(0, 0)
        }
    }
}
