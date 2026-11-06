// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

/// @title DevnetSettle
/// @notice A compact stand-in for ShieldedPool on the ERC-8421 alt mempool devnet (chain 1337), whose
/// 60M block gas limit cannot hold the 59 KB terminal proof verifier or the 50 KB pool (each needs
/// 77M to 90M gas of code deposit). It keeps the settle's exact transaction shape: PoolAccount's VERIFY
/// frame, the PQ paymaster's leanSPHINCS VERIFY frame and a SENDER call to `settle` with the real
/// terminal proof and both spends' 82 public fields. It checks that both spends name this pool and
/// scope and that their nullifiers are fresh, then records them. It does NOT verify the terminal
/// proof: that happens on chain 8141, where the full ShieldedPool runs.
/// @dev Runs by DELEGATECALL from PoolAccount, so its state lives in the pool account.
contract DevnetSettle {
    uint256 private constant FIELDS = 82;
    uint256 private constant POOL_ID = 0;
    uint256 private constant SCOPE = 8;
    uint256 private constant NULLIFIER = 66;

    bytes32 private immutable poolIdHash;
    bytes32 private immutable scopeHash;

    bool public initialized;
    uint256 public settles;
    mapping(uint256 => bool) public spent;

    event Settled(uint256 indexed nullifierA, uint256 indexed nullifierB, bytes32 proofHash, uint256 settles);

    error AlreadyInitialized();
    error NotPool();
    error BadFields();
    error WrongPool();
    error NullifierSpent();

    constructor(uint256[8] memory poolId, uint256[8] memory scope) {
        poolIdHash = keccak256(abi.encode(poolId));
        scopeHash = keccak256(abi.encode(scope));
    }

    /// Called once by PoolAccount's initcode.
    function initialize() external {
        if (initialized) revert AlreadyInitialized();
        initialized = true;
    }

    /// The settle frame: same signature and selector (0xf4b589c7) as ShieldedPool.settle.
    function settle(bytes calldata proofCall, uint256[] calldata a, uint256[] calldata b) external {
        if (msg.sender != address(this)) revert NotPool();
        uint256 na = _check(a);
        uint256 nb = _check(b);
        if (na == nb || spent[na] || spent[nb]) revert NullifierSpent();
        spent[na] = true;
        spent[nb] = true;
        settles += 1;
        emit Settled(na, nb, keccak256(proofCall), settles);
    }

    function _check(uint256[] calldata f) private view returns (uint256 nullifier) {
        if (f.length != FIELDS) revert BadFields();
        if (keccak256(abi.encode(_eight(f, POOL_ID))) != poolIdHash) revert WrongPool();
        if (keccak256(abi.encode(_eight(f, SCOPE))) != scopeHash) revert WrongPool();
        uint256[8] memory n = _eight(f, NULLIFIER);
        for (uint256 i = 0; i < 8; ++i) {
            nullifier |= n[i] << (32 * i);
        }
    }

    function _eight(uint256[] calldata f, uint256 at) private pure returns (uint256[8] memory out) {
        for (uint256 i = 0; i < 8; ++i) {
            out[i] = f[at + i];
        }
    }
}
