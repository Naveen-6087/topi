// SPDX-License-Identifier: MIT OR Apache-2.0
// This file is GENERATED from codegen/ShieldedPool.tmpl.sol by codegen/generate.mjs. Edit the template.
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
    bytes internal constant POOL_ZEROS = hex"000000000000000000000000000000000000000000000000000000000000000072d67c085d2ef28a77170ead1ecfe08e331654102f4d112a29e3b7a23327acb457aacbce2d543a1d52178df738c5d57b25c6efb179709181109e02fe64c1ee9c6bebed5163b7f2525d15deb353ee25cb3c66235a1f24e61a22aa8e470ac35a2a7311b89d733634ef04b05fcf516c2ad557d69ba4537944a7376ed1367a62194d1adf5c4c13b4fae3570f0ae5768ee3433f90de3f4dd4effb375db20724d61abb5d4cf0174a3d536e5bb793b4284b50064bef99c57813967c10476cb4356aea1a09d39dd04983595b37c8b65f7401c2ba22e5dc8352ce891a06af7ae71f369a1c5350dbab1ca017767274c6d442c3753d7288033a22b8946e09c4d92e0b0fb49972ae18dc69760e7270a68bad00adf3df7713ca0016e1582651b2718263136ff95507eaf148423efb3392a8156a5db2ed7bf14c0b7115d59839b84e7f6e9eb514042c5a6b13c299be0c654be818726de4182c8f9f36c398b55f5023b2599be5de6f9f68514b54ebb03ab0ee0f249484307514b3c2778818e26be7a333379d41b7077a89d7246c7337617c820d2f5c0e4b1c70d19d569212087d532aee03fb08876b4ed41b6f2814790e41930d1ac0f085139912b876da8a3441e921ff03d47cb611d5f811056b48673526d2701f816d945f8a45e93c1c3a0009066cb049d9466823f0e01d327e785f71990323499539236c5114a5557379b43b0cbfdc36e0deac25544df23653aa1814787996727c118c2727d29868a4aaf25f1bb9f94cc533c07ae27c293dfb039620a16e9f5a6c07f42349564e77de2c6268b93eda186f9f4d54ffecf24b9b952442d527d474ef8899367116dc5102665448f99cd0202cca9e2ea4b26347aa3f325d9ad330008e3cf079ceb03e0112dc616535a12573c827e16227a022680fe02d5344dda325d70a9963de11894676faff2cfe3ad616df0cb464d71e0f0a2585595195d1f54e0336887bba92562b63ad280739f1ad0750d6652d913ff93339dfc361eb4bf448efc364239a9a580a151d3815ce2edc28f0309d3349b13f4844d99a4478f70e37bb76df68fd5b681c5f32212f51ab477349651f2c4ec182755607246db7ac7f72dc2aad5e5fe8ed16bf427219a39a685437150d38762e5a3ccd1f1c3357e0553399e1b60f447296127077ed121b6e2850cb87a142e096f15144e75c3de31ec8012613ce21e3eb56748931911c77c199766e3536486e453e13237952522ce3fa6690767968cc36883c93db523996b0bd0fcc12be084447c15b0bad4b4b6987554d46872a0d4ec8d340d8cb620d43e41b5d5d4efa1c2a993260a29c7c73ec5f6a76b007757aee41800f1a645d611bebf544f10f5228ca0bea0ec63d86431f59a04b2ac749178efa9f01a588775349b1647796f9cc02c9bb782566fdbd669560723b24d8a429a380f22dacabcb599df1bc75ca996f";
    bytes internal constant ASSOCIATION_ZEROS = hex"00000000000000000000000000000000000000000000000000000000000000001cd8531727d9bf411094a095755113f5190ba6f337ae71596da97ca93ae56cc5729e80527c86305672689f7a375eb550633d64120da7f688254865783d04800b57ae8d212bee4a117befad144fde787c763b6915788701e65575f4bd254e487367b7a42337214bd83a84916c46631ff53b729d855e4803e96d90470171ea42d00b56a2be50058cd376244056089d451d4b648f422e317afd75cded5d3bdec3fb422641d975989d4a37aeefee6f6f515c7d68572438d1b4b60c6e8b5257d089764645a07f2f865a545673d47e5f1646e331c02a4a360924c1779c8eb648f30305334efe6920adea1f7eff8c445fbc20b77581e1be7ed71e601e71a6f80f7295664f68aae75c0206e062e5a4fe5f7afe64109ec06133e782e667adf04d534e4d6558c0606771dfc63d5097b3760cd0f856087a21a3455408bd45e541cc5c6b7b8852e039996f0edc3d52896a946b57456e6b4f0dac020b72195db017fb411f0b057ef4668b33d8dafe29ab70301e69492d527de53830bb0aaf5924e33e6d8e85a54fc53cef380e080b520713b665f7f83e158ec384287e31b0516c8e972388f43a2db3753c033b758f1b7ff3c91cd6f2107753528116a35b1074ac2ed2127be23d4fd12c704d051ead0e65a5034f74373c530678905c8cf7871b99bc292dfff14a297fee443157648e043cfa1c116eb6bc47b47120663c8369102d2aaa26702712079828b35ab1c83c6706a1813a02f936541138145a49c084576042d6256d741a6937d15276d21583214e14c94e6ded0654a699995489e86b4fb4f9bb3af30f136aa8f8661fee374b66ec7f957ba26e4202e54cd2602ff1ff7dde00145824939331199a93022b748721f1f5f56fa1e96371fb5329685135146771254101a0b848335c3605054bcc9d47ebaf386d6bb0796d0f6dee0e9b7f342d9ad32623f7ef0d254287332fca83a72806b60b6b154a9616409d2c382991203d2a98f841de727056fd13d45f65d79935f2c9fa23da2a453807e25f0bc844176cb61e5117dd231158a8a7253504728a0b1b4ea645e5d1ea7126e3e7571b64f0106c19161a24d4986daf557e0c25ea0d2c28a0c566fdd85b630b53863a0c3a3945944c557a541e3e51c1d42f57f5fe904ffd745a0e8dd3c14c9efde474eee63d60d6ed02397530fd3517face6e949a1f0593f0b31e6fd0d6045f547857efa84362444d894bbc20624846b1913d64ee2f0933d8f43bda42691aea269e4361d763003fec57479f0b6117cd2b995b448c6a18ca8c825574018877d56b6d65f4332350809c850b4a0d3d62539491287651f93230bbe3780abb087470a5f74b25655d6a6af37c0e8df86a34308fd167492b0729ec49702bd177937b85353620d544485052312271e61d4926aa6b832adfa9ce139863496bac32a2374a41852c7a4edd5d03a8a70b6d8b3e";

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
