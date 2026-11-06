// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

/// @notice Minimal substring search for tests that check source files against each other.
library Text {
    function contains(string memory haystack, string memory needle) internal pure returns (bool) {
        bytes memory h = bytes(haystack);
        bytes memory n = bytes(needle);
        if (n.length == 0) return true;
        if (n.length > h.length) return false;
        for (uint256 i = 0; i + n.length <= h.length; ++i) {
            uint256 j = 0;
            while (j < n.length && h[i + j] == n[j]) ++j;
            if (j == n.length) return true;
        }
        return false;
    }
}
