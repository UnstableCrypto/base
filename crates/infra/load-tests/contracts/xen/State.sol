// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title State
/// @notice Minimal XEN-like state contract used by the load test.
/// Stores a per-msg.sender uint256 keyed in a single mapping. Each
/// `claimRank` call by a fresh msg.sender (e.g. a freshly-CREATE2'd
/// `Worker`) writes a unique storage slot, producing one storage-trie
/// entry per call.
contract State {
    mapping(address => uint256) public ranks;

    function claimRank(uint256 term) external {
        ranks[msg.sender] = term;
    }
}
