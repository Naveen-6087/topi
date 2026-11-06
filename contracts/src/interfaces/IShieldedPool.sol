// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

/// @title IShieldedPool
/// @notice External interface of the shielded pool (ShieldedPool, reached through the pool account).
/// Digests are 8 KoalaBear elements packed 32 bits each into one word (element i at bits 32i..32i+31).
interface IShieldedPool {
    /// @notice A deposit added pool leaves [note, 0] at `leafIndex` and association leaf `label` at `index`.
    event Deposited(
        uint256 indexed index,
        uint256 leafIndex,
        uint256 note,
        uint256 label,
        uint256 amount,
        uint256 poolRoot,
        uint256 associationRoot
    );

    /// @notice A settlement spent two notes and added their change commitments as pool leaves
    /// [changeA, changeB] at `leafIndex`.
    event Settled(
        uint256 leafIndex, uint256 nullifierA, uint256 nullifierB, uint256 changeA, uint256 changeB, uint256 poolRoot
    );

    error AlreadyInitialized();
    error BadAmount();
    error NonCanonical();
    error NotPool();
    error WrongSelector();
    error ProofRejected();
    error RootMismatch();
    error WrongPool();
    error UnknownRoot();
    error AlreadySpent();
    error NotTestbed();
    error BadContext();
    error PayoutFailed();

    /// @notice Records the empty roots. Called once by the pool account's initcode.
    function initialize() external;

    /// @notice Deposits msg.value under `precommitment` = hash_fields(PRECOMMIT, nullifier || secret).
    /// @return index The deposit index (also the association tree leaf index).
    function deposit(uint256[8] calldata precommitment) external payable returns (uint256 index);

    /// @notice Settles two spends proven by one leanVM terminal proof. Only callable by the pool itself
    /// (the SENDER frame of a frame transaction the pool account validated).
    /// @param proofCall Full calldata for ITerminalVerifier.verifyGroupedLogupV1.
    /// @param a The first spend's 82 transaction fields, in circuit order.
    /// @param b The second spend's 82 transaction fields, in circuit order.
    function settle(bytes calldata proofCall, uint256[] calldata a, uint256[] calldata b) external;

    /// @notice Testbed pools only: marks roots as known (operator only).
    function testbedSeedRoots(uint256 pool, uint256 association) external;

    function terminal() external view returns (address);
    function operator() external view returns (address);
    function testbed() external view returns (bool);
    function poolId() external view returns (uint256);
    function scope() external view returns (uint256);
    function initialized() external view returns (bool);
    function knownPoolRoot(uint256 root) external view returns (bool);
    function knownAssociationRoot(uint256 root) external view returns (bool);
    function spent(uint256 nullifier) external view returns (bool);
    function deposits() external view returns (uint256);
    function poolRoot() external view returns (uint256);
    function associationRoot() external view returns (uint256);
    function nextPoolIndex() external view returns (uint256);
}
