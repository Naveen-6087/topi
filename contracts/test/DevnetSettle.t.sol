// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.28;

import { Test } from "forge-std/Test.sol";
import { DevnetSettle } from "../src/devnet/DevnetSettle.sol";

contract DevnetSettleTest is Test {
    DevnetSettle s;
    uint256[8] poolId = [uint256(1), 2, 3, 4, 5, 6, 7, 8];
    uint256[8] scope = [uint256(11), 12, 13, 14, 15, 16, 17, 18];

    function setUp() public {
        s = new DevnetSettle(poolId, scope);
        s.initialize();
    }

    function fields(uint256 nullifierSeed) internal view returns (uint256[] memory f) {
        f = new uint256[](82);
        for (uint256 i = 0; i < 8; ++i) {
            f[i] = poolId[i];
            f[8 + i] = scope[i];
            f[66 + i] = nullifierSeed + i;
        }
    }

    function test_settles_once_and_refuses_replays() public {
        vm.prank(address(s));
        s.settle(hex"0102", fields(100), fields(200));
        assertEq(s.settles(), 1);
        uint256 na = 0;
        for (uint256 i = 0; i < 8; ++i) na |= (100 + i) << (32 * i);
        assertTrue(s.spent(na));
        vm.prank(address(s));
        vm.expectRevert(DevnetSettle.NullifierSpent.selector);
        s.settle(hex"0102", fields(100), fields(300));
    }

    function test_refuses_other_pools_callers_and_shapes() public {
        uint256[] memory other = fields(400);
        other[0] = 99;
        vm.prank(address(s));
        vm.expectRevert(DevnetSettle.WrongPool.selector);
        s.settle(hex"", other, fields(500));
        vm.expectRevert(DevnetSettle.NotPool.selector);
        s.settle(hex"", fields(600), fields(700));
        vm.prank(address(s));
        vm.expectRevert(DevnetSettle.BadFields.selector);
        s.settle(hex"", new uint256[](3), fields(800));
        vm.expectRevert(DevnetSettle.AlreadyInitialized.selector);
        s.initialize();
    }
}
