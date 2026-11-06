// SPDX-License-Identifier: MIT OR Apache-2.0
/// @title ClassicPaymaster
/// @notice Pays gas for shielded pool transactions that the exit operator signed with secp256k1
/// (ECDSA). The exit signs only after verifying both client proofs natively, so it risks only the gas
/// of transactions it vetted. Superseded by PQPaymaster; kept for the first pool deployment.
/// A call whose calldata is exactly 4 zero bytes is the VERIFY entry: it approves PAYMENT (scope 1).
/// Plain transfers fund the paymaster; the operator can withdraw with calldata 0x3ccfd60b (withdraw()).
/// @dev Runtime layout: this code, then the pool address and the operator address (32 bytes each).
/// Modelled in lean/PoolModel/Frames.lean (paymasterPolicy).
object "ClassicPaymaster" {
    code {
        let rsize := datasize("runtime")
        datacopy(0, dataoffset("runtime"), rsize)
        // The deployer appends the pool and operator addresses as two 32-byte words.
        codecopy(rsize, sub(codesize(), 64), 64)
        return(0, add(rsize, 64))
    }

    object "runtime" {
        code {
            function txParam(param) -> value { value := verbatim_1i_1o(hex"B0", param) }
            function frameParam(frameIndex, param) -> value {
                value := verbatim_2i_1o(hex"B3", frameIndex, param)
            }
            function sigParam(signatureIndex, param) -> value {
                value := verbatim_2i_1o(hex"B4", signatureIndex, param)
            }
            // APPROVE with an empty return region and scope 1, APPROVE_PAYMENT.
            function approvePayment() { verbatim_3i_0o(hex"AA", 0, 0, 1) }

            function word(i) -> w {
                codecopy(0, add(sub(codesize(), 64), mul(i, 32)), 32)
                w := mload(0)
            }
            function fail(code) {
                mstore(0, code)
                revert(0, 32)
            }

            if and(eq(calldatasize(), 4), iszero(shr(224, calldataload(0)))) {
                let pool := word(0)
                let operator := word(1)
                // Only the configured pool's transactions, with this paymaster as frame 1.
                if iszero(eq(txParam(0x02), pool)) { fail(1) }
                if iszero(eq(txParam(0x0A), 1)) { fail(2) }
                if iszero(eq(frameParam(1, 0x00), address())) { fail(3) }
                if iszero(eq(frameParam(1, 0x02), 1)) { fail(4) }
                if iszero(eq(frameParam(1, 0x03), 1)) { fail(5) }
                // Exactly one signature: secp256k1 (scheme 1) by the operator over the whole
                // transaction's signature hash (empty msg), which the protocol validated.
                if iszero(eq(txParam(0x0B), 1)) { fail(6) }
                if iszero(eq(sigParam(0, 0), operator)) { fail(7) }
                if iszero(eq(sigParam(0, 1), 1)) { fail(8) }
                if sigParam(0, 2) { fail(9) }
                approvePayment()
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
