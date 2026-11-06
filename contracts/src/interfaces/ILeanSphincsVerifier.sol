// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

/// @title ILeanSphincsVerifier
/// @notice The shared leanSPHINCS (hash-based, post-quantum) signature verifier that PQPaymaster.yul
/// calls with STATICCALL. Selector 0x2bf6eda8. A leanSPHINCS signature is 5684 bytes.
interface ILeanSphincsVerifier {
    /// @notice Returns true iff `sig` is a valid signature of `message` under the public key
    /// (`pkSeed`, `pkRoot`).
    function verify(bytes32 pkSeed, bytes32 pkRoot, bytes32 message, bytes calldata sig)
        external
        view
        returns (bool);
}
