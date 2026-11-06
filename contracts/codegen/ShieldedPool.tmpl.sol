// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

import { KoalaPoseidon2x24 as H } from "./hash/KoalaPoseidon2x24.sol";
import { StatementBinding } from "./StatementBinding.sol";

/// @title PackedTree
/// @notice Depth-32 incremental Merkle trees over KoalaBear digests (8 elements, packed 32 bits each
/// into one word), with spartan-whir's withdrawal node hash `hash_fields(domain, left || right)`.
library PackedTree {
    uint256 internal constant DEPTH = 32;

    struct Tree {
        uint256[DEPTH] filled;
        uint256 next;
        uint256 root;
    }

    function zero(bytes memory zeros, uint256 level) internal pure returns (uint256 w) {
        assembly ("memory-safe") {
            w := mload(add(add(zeros, 32), shl(5, level)))
        }
    }

    /// Append one leaf. Returns the new root.
    function insert(Tree storage t, uint256 domain, bytes memory zeros, uint256[8] memory leaf)
        internal
        returns (uint256)
    {
        return _insertFrom(t, domain, zeros, leaf, t.next++, 0);
    }

    /// Append two leaves at an even index: one hash for the pair, then the path from level 1.
    function insertPair(Tree storage t, uint256 domain, bytes memory zeros, uint256[8] memory a, uint256[8] memory b)
        internal
        returns (uint256)
    {
        uint256 index = t.next;
        require(index & 1 == 0, "odd index");
        t.next = index + 2;
        return _insertFrom(t, domain, zeros, H.nodeIn(domain, a, b), index >> 1, 1);
    }

    function _insertFrom(
        Tree storage t,
        uint256 domain,
        bytes memory zeros,
        uint256[8] memory node,
        uint256 at,
        uint256 fromLevel
    ) private returns (uint256 root) {
        for (uint256 level = fromLevel; level < DEPTH; ++level) {
            if (at & 1 == 0) {
                t.filled[level] = pack(node);
                node = H.nodeIn(domain, node, unpack(zero(zeros, level)));
            } else {
                node = H.nodeIn(domain, unpack(t.filled[level]), node);
            }
            at >>= 1;
        }
        root = pack(node);
        t.root = root;
    }

    function pack(uint256[8] memory d) internal pure returns (uint256 w) {
        for (uint256 i = 0; i < 8; ++i) w |= d[i] << (32 * i);
    }

    function unpack(uint256 w) internal pure returns (uint256[8] memory d) {
        for (uint256 i = 0; i < 8; ++i) d[i] = (w >> (32 * i)) & 0xffffffff;
    }
}

/// @title ShieldedPool
/// @notice Logic of the post-quantum shielded pool. Runs by DELEGATECALL from the pool account
/// (yul/PoolAccount.yul), so all state lives in the pool account's storage; the account's VERIFY entry
/// enforces the EIP-8141 frame policy.
/// @dev Spend relation: spartan-whir's privacy-pools withdrawal, lean statement (public input = the
/// 8-element transaction digest). A spend is its 82 transaction fields in circuit order: pool 0..8,
/// scope 8..16, context 16..24, recipient 24..34 (16-bit limbs), withdrawn 34..42, fee 42..50, pool root
/// 50..58, association root 58..66, nullifier hash 66..74, change commitment 74..82.
///
/// Testbed mode (immutable, chosen at deployment) exists only so recorded proofs of spartan-whir's
/// deterministic samples can be settled without real notes: the operator may mark roots as known,
/// payouts are capped at 1 wei, and the fee goes to the operator instead of the context's address.
/// A non-testbed pool has none of these.
contract ShieldedPool {
    using PackedTree for PackedTree.Tree;

    uint256 internal constant P = 2_130_706_433;
    uint256 internal constant NOTE = 2;
    uint256 internal constant POOL_NODE = 4;
    uint256 internal constant ASSOCIATION_NODE = 5;
    uint256 internal constant TRANSACTION = 6;
    uint256 internal constant LABEL = 7; // the pool's own domain for deposit labels (unused by the circuit)
    bytes4 internal constant VERIFY_SELECTOR = 0x5801ce09; // verifyGroupedLogupV1
    uint256 internal constant ROOT_OFFSET = 100; // proof.applicationRoot in that calldata
    bytes internal constant POOL_ZEROS = hex"__POOL_ZEROS__";
    bytes internal constant ASSOCIATION_ZEROS = hex"__ASSOCIATION_ZEROS__";

    address public immutable terminal;
    address public immutable operator;
    bool public immutable testbed;
    uint256 public immutable poolId; // packed 8 elements
    uint256 public immutable scope; // packed 8 elements
    /// Poseidon2 state of hash_fields(TRANSACTION, ·) after its first block, which is always this pool's
    /// pool || scope: 24 elements packed 8 per word. Saves one permutation per spend.
    uint256 internal immutable txPrefix0;
    uint256 internal immutable txPrefix1;
    uint256 internal immutable txPrefix2;

    // Storage (in the pool account).
    bool public initialized;
    PackedTree.Tree internal poolTree;
    PackedTree.Tree internal associationTree;
    mapping(uint256 => bool) public knownPoolRoot;
    mapping(uint256 => bool) public knownAssociationRoot;
    mapping(uint256 => bool) public spent; // packed nullifier hash
    uint256 public deposits;

    /// Every leaf of both trees is in these events, so a wallet can rebuild the trees from logs alone:
    /// a deposit adds pool leaves [note, 0] at `leafIndex` and association leaf `label` at `index`;
    /// a settle adds pool leaves [changeA, changeB] at `leafIndex`. Digests are packed (32 bits each).
    event Deposited(
        uint256 indexed index,
        uint256 leafIndex,
        uint256 note,
        uint256 label,
        uint256 amount,
        uint256 poolRoot,
        uint256 associationRoot
    );
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

    constructor(address terminal_, address operator_, bool testbed_, uint256[8] memory pool_, uint256[8] memory scope_) {
        terminal = terminal_;
        operator = operator_;
        testbed = testbed_;
        poolId = PackedTree.pack(pool_);
        scope = PackedTree.pack(scope_);
        uint256[24] memory s;
        for (uint256 i = 0; i < 8; ++i) {
            s[i] = pool_[i];
            s[8 + i] = scope_[i];
        }
        s[16] = TRANSACTION;
        s[17] = 82;
        s[18] = 1;
        H.permute(s);
        uint256[8] memory w;
        for (uint256 i = 0; i < 8; ++i) w[i] = s[i];
        txPrefix0 = PackedTree.pack(w);
        for (uint256 i = 0; i < 8; ++i) w[i] = s[8 + i];
        txPrefix1 = PackedTree.pack(w);
        for (uint256 i = 0; i < 8; ++i) w[i] = s[16 + i];
        txPrefix2 = PackedTree.pack(w);
    }

    /// hash_fields(TRANSACTION, f) for an 82-field spend, resuming after the constant pool || scope block.
    /// A spend whose first 16 fields are not this pool's gets a different digest than its proof commits to.
    function _txDigest(uint256[] calldata f) internal view returns (uint256[8] memory out) {
        uint256[24] memory s;
        uint256[8] memory w = PackedTree.unpack(txPrefix0);
        for (uint256 i = 0; i < 8; ++i) s[i] = w[i];
        w = PackedTree.unpack(txPrefix1);
        for (uint256 i = 0; i < 8; ++i) s[8 + i] = w[i];
        w = PackedTree.unpack(txPrefix2);
        for (uint256 i = 0; i < 8; ++i) s[16 + i] = w[i];
        for (uint256 off = 16; off < 82; off += 16) {
            for (uint256 i = 0; i < 16 && off + i < 82; ++i) {
                uint256 v = f[off + i];
                if (v >= P) revert NonCanonical();
                s[i] = addmod(s[i], v, P);
            }
            H.permute(s);
        }
        for (uint256 i = 0; i < 8; ++i) out[i] = s[i];
    }

    /// Called once by the pool account's initcode (DELEGATECALL): records the empty roots.
    function initialize() external {
        if (initialized) revert AlreadyInitialized();
        initialized = true;
        uint256 p = PackedTree.zero(POOL_ZEROS, 32);
        uint256 a = PackedTree.zero(ASSOCIATION_ZEROS, 32);
        poolTree.root = p;
        associationTree.root = a;
        knownPoolRoot[p] = true;
        knownAssociationRoot[a] = true;
    }

    // ----- deposits -----

    /// Public deposit of msg.value under `precommitment` = hash_fields(PRECOMMIT, nullifier || secret),
    /// which only the depositor can open. The pool assigns the label and builds the note:
    ///   label = hash_fields(LABEL, scope || index), note = hash_fields(NOTE, pool || label || amount || precommitment)
    /// The note goes into the pool tree as a pair with a zero leaf (keeps settle's paired inserts aligned);
    /// the label goes into the association tree (every deposit is associated).
    function deposit(uint256[8] calldata precommitment) external payable returns (uint256 index) {
        if (msg.value == 0 || msg.value >= 1 << 128) revert BadAmount();
        for (uint256 i = 0; i < 8; ++i) {
            if (precommitment[i] >= P) revert NonCanonical();
        }
        index = deposits++;
        uint256[] memory labelIn = new uint256[](9);
        uint256[8] memory s = PackedTree.unpack(scope);
        for (uint256 i = 0; i < 8; ++i) labelIn[i] = s[i];
        labelIn[8] = index;
        uint256[8] memory label = H.hashFields(LABEL, labelIn);

        uint256[] memory noteIn = new uint256[](32);
        uint256[8] memory pool_ = PackedTree.unpack(poolId);
        for (uint256 i = 0; i < 8; ++i) {
            noteIn[i] = pool_[i];
            noteIn[8 + i] = label[i];
            noteIn[16 + i] = (msg.value >> (16 * i)) & 0xffff;
            noteIn[24 + i] = precommitment[i];
        }
        uint256[8] memory note = H.hashFields(NOTE, noteIn);
        uint256[8] memory none;
        uint256 leafIndex = poolTree.next;
        uint256 poolRoot = poolTree.insertPair(POOL_NODE, POOL_ZEROS, note, none);
        uint256 associationRoot = associationTree.insert(ASSOCIATION_NODE, ASSOCIATION_ZEROS, label);
        knownPoolRoot[poolRoot] = true;
        knownAssociationRoot[associationRoot] = true;
        emit Deposited(
            index, leafIndex, PackedTree.pack(note), PackedTree.pack(label), msg.value, poolRoot, associationRoot
        );
    }

    // ----- settlement (SENDER frame of a pool transaction) -----

    /// Settles two spends proven by one leanVM terminal proof. Only the pool itself may call it, i.e.
    /// a SENDER frame of a frame transaction whose sender is the pool and whose VERIFY policy passed.
    /// Any failure reverts the frame: no nullifier is consumed and no value moves.
    function settle(bytes calldata proofCall, uint256[] calldata a, uint256[] calldata b) external {
        if (msg.sender != address(this)) revert NotPool();
        if (a.length != 82 || b.length != 82) revert NonCanonical();
        if (bytes4(proofCall[:4]) != VERIFY_SELECTOR) revert WrongSelector();
        (bool ok, bytes memory ret) = terminal.staticcall(proofCall);
        if (!ok || ret.length != 32 || abi.decode(ret, (uint256)) != 1) revert ProofRejected();

        uint256[8] memory want = StatementBinding.applicationRoot(
            StatementBinding.leanStatementDigest(_txDigest(a)),
            StatementBinding.leanStatementDigest(_txDigest(b))
        );
        for (uint256 i = 0; i < 8; ++i) {
            if (uint256(bytes32(proofCall[ROOT_OFFSET + 32 * i:ROOT_OFFSET + 32 * (i + 1)])) != want[i]) {
                revert RootMismatch();
            }
        }

        uint256 na = _spend(a);
        uint256 nb = _spend(b);
        uint256 leafIndex = poolTree.next;
        uint256 poolRoot = poolTree.insertPair(POOL_NODE, POOL_ZEROS, _digest(a, 74), _digest(b, 74));
        knownPoolRoot[poolRoot] = true;
        emit Settled(leafIndex, na, nb, _packed(a, 74), _packed(b, 74), poolRoot);
    }

    function _spend(uint256[] calldata f) internal returns (uint256 nullifier) {
        if (_packed(f, 0) != poolId || _packed(f, 8) != scope) revert WrongPool();
        if (!knownPoolRoot[_packed(f, 50)] || !knownAssociationRoot[_packed(f, 58)]) revert UnknownRoot();
        nullifier = _packed(f, 66);
        if (spent[nullifier]) revert AlreadySpent();
        spent[nullifier] = true;

        address recipient = address(uint160(_limbs(f, 24, 10)));
        uint256 withdrawn = _limbs(f, 34, 8);
        uint256 fee = _limbs(f, 42, 8);
        address feeTo = testbed ? operator : _contextAddress(f);
        if (testbed) {
            withdrawn = withdrawn > 0 ? 1 : 0;
            fee = fee > 0 ? 1 : 0;
        }
        _pay(recipient, withdrawn);
        _pay(feeTo, fee);
    }

    /// The fee recipient the spender committed to: context = 6 limbs of 28 bits (little endian) holding
    /// a 160-bit address, then two zero elements. The circuit proves the spender chose this context.
    function _contextAddress(uint256[] calldata f) internal pure returns (address) {
        uint256 v;
        for (uint256 i = 0; i < 6; ++i) {
            if (f[16 + i] >> 28 != 0) revert BadContext();
            v |= f[16 + i] << (28 * i);
        }
        if (f[22] != 0 || f[23] != 0 || v >> 160 != 0) revert BadContext();
        return address(uint160(v));
    }

    function _pay(address to, uint256 amount) internal {
        if (amount == 0) return;
        (bool ok,) = payable(to).call{ value: amount }("");
        if (!ok) revert PayoutFailed();
    }

    function _packed(uint256[] calldata f, uint256 start) internal pure returns (uint256 w) {
        for (uint256 i = 0; i < 8; ++i) {
            if (f[start + i] >= P) revert NonCanonical();
            w |= f[start + i] << (32 * i);
        }
    }

    function _limbs(uint256[] calldata f, uint256 start, uint256 count) internal pure returns (uint256 v) {
        for (uint256 i = 0; i < count; ++i) {
            if (f[start + i] >> 16 != 0) revert NonCanonical();
            v |= f[start + i] << (16 * i);
        }
    }

    function _digest(uint256[] calldata f, uint256 start) internal pure returns (uint256[8] memory d) {
        for (uint256 i = 0; i < 8; ++i) d[i] = f[start + i];
    }

    // ----- views and testbed -----

    function poolRoot() external view returns (uint256) {
        return poolTree.root;
    }

    function associationRoot() external view returns (uint256) {
        return associationTree.root;
    }

    function nextPoolIndex() external view returns (uint256) {
        return poolTree.next;
    }

    /// Testbed only: mark a root as known so recorded proofs over spartan-whir's sample trees can settle.
    function testbedSeedRoots(uint256 pool_, uint256 association_) external {
        if (!testbed || msg.sender != operator) revert NotTestbed();
        knownPoolRoot[pool_] = true;
        knownAssociationRoot[association_] = true;
    }
}
