// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Worker} from "./Worker.sol";

/// @title Multicall
/// @notice Loops `count` CREATE2 deployments of `Worker`, where each per-iteration
/// salt is `keccak256(abi.encodePacked(baseSalt, i))`. Mirrors the trie-load
/// shape of XEN's `bulkClaimRank`: one tx produces N new account-trie entries
/// (the workers) and N unique storage-trie writes (per-worker slots in `state`).
contract Multicall {
    function bulkClaimRank(
        address state,
        uint256 term,
        uint256 baseSalt,
        uint256 count
    ) external {
        bytes memory initCode = abi.encodePacked(
            type(Worker).creationCode,
            abi.encode(state, term)
        );
        for (uint256 i = 0; i < count; i++) {
            bytes32 salt = keccak256(abi.encodePacked(baseSalt, i));
            assembly {
                let addr := create2(0, add(initCode, 0x20), mload(initCode), salt)
                if iszero(addr) {
                    revert(0, 0)
                }
            }
        }
    }
}
