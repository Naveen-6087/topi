// SPDX-License-Identifier: MIT OR Apache-2.0
/// @title PoolAccount
/// @notice The shielded pool's account code: an EIP-8141 validation shell in front of ShieldedPool.
/// A call whose calldata is exactly 4 zero bytes is the VERIFY entry. It checks the transaction's
/// frame layout and approves EXECUTION only (scope 2): the pool never pays gas, a paymaster does.
/// Every other call is DELEGATECALLed to ShieldedPool, which keeps all state in this account.
/// @dev Runtime layout: this code, then the ShieldedPool address as one 32-byte word.
/// Conventions (TXPARAM, FRAMEPARAM indices, verbatim argument order) follow soispoke's
/// ShieldedPoolDispatcher.yul for the same chain (8141, EIP-8141 rev b75cbe6115).
/// The policy is transcribed check by check, with the same failure codes, in lean/PoolModel/Frames.lean.
object "PoolAccount" {
    code {
        let rsize := datasize("runtime")
        // The deployer appends the logic address to this initcode as one 32-byte word.
        codecopy(0, sub(codesize(), 32), 32)
        let logic := mload(0)
        // Record the empty roots in this account's storage: logic.initialize() by DELEGATECALL.
        mstore(0, shl(224, 0x8129fc1c))
        if iszero(delegatecall(gas(), logic, 0, 4, 0, 0)) { revert(0, 0) }
        datacopy(0, dataoffset("runtime"), rsize)
        mstore(rsize, logic)
        return(0, add(rsize, 32))
    }

    object "runtime" {
        code {
            // EIP-8141 opcodes, which solc cannot emit; verbatim puts its first argument on top.
            function txParam(param) -> value { value := verbatim_1i_1o(hex"B0", param) }
            function frameParam(frameIndex, param) -> value {
                value := verbatim_2i_1o(hex"B3", frameIndex, param)
            }
            function frameDataLoad(frameIndex, offset) -> value {
                value := verbatim_2i_1o(hex"B1", offset, frameIndex)
            }
            // APPROVE with an empty return region and scope 2, APPROVE_EXECUTION.
            function approveExecution() { verbatim_3i_0o(hex"AA", 0, 0, 2) }

            function fail(code) {
                mstore(0, code)
                revert(0, 32)
            }

            // settle(bytes,uint256[],uint256[])
            function settleSelector() -> s { s := 0xf4b589c7 }

            function verifyFrameApprove() {
                // The pool is the sender; exactly three frames; this entry runs as frame 0; no blobs.
                if iszero(eq(txParam(0x02), address())) { fail(1) }
                if iszero(eq(txParam(0x09), 3)) { fail(2) }
                if txParam(0x0A) { fail(3) }
                if txParam(0x07) { fail(4) }
                // At least one EIP-8250 nonce key at sequence zero: spends are order-free; replay
                // protection for value is the pool's nullifier set, not the nonce key.
                if iszero(txParam(0x0E)) { fail(5) }
                if txParam(0x01) { fail(6) }

                // Frame 0: this VERIFY frame, targeting the pool, flags 2 (execution only), zero value.
                if iszero(eq(frameParam(0, 0x00), address())) { fail(10) }
                if iszero(eq(frameParam(0, 0x02), 1)) { fail(11) }
                if iszero(eq(frameParam(0, 0x03), 2)) { fail(12) }
                if frameParam(0, 0x08) { fail(13) }

                // Frame 1: the payer's VERIFY frame (flags 1, payment only), at another account.
                // The pool does not care who pays, only that it is not the pool.
                let payer := frameParam(1, 0x00)
                if or(iszero(payer), eq(payer, address())) { fail(20) }
                if iszero(eq(frameParam(1, 0x02), 1)) { fail(21) }
                if iszero(eq(frameParam(1, 0x03), 1)) { fail(22) }
                if frameParam(1, 0x08) { fail(23) }

                // Frame 2: the only code that runs as the pool: one SENDER call to the pool's own
                // settle, zero value, zero flags (no atomic batch). DEFAULT frames are excluded by the
                // fixed frame count, so nothing else can reach the pool with the pool as caller.
                if iszero(eq(frameParam(2, 0x00), address())) { fail(30) }
                if iszero(eq(frameParam(2, 0x02), 2)) { fail(31) }
                if frameParam(2, 0x03) { fail(32) }
                if frameParam(2, 0x08) { fail(33) }
                if lt(frameParam(2, 0x04), 4) { fail(34) }
                if iszero(eq(shr(224, frameDataLoad(2, 0)), settleSelector())) { fail(35) }

                approveExecution()
            }

            if and(eq(calldatasize(), 4), iszero(shr(224, calldataload(0)))) {
                verifyFrameApprove()
                stop()
            }

            // Delegate everything else to the logic (address stored after the runtime code).
            codecopy(0, sub(codesize(), 32), 32)
            let target := mload(0)
            calldatacopy(0, 0, calldatasize())
            let ok := delegatecall(gas(), target, 0, calldatasize(), 0, 0)
            returndatacopy(0, 0, returndatasize())
            switch ok
            case 0 { revert(0, returndatasize()) }
            default { return(0, returndatasize()) }
        }
    }
}
