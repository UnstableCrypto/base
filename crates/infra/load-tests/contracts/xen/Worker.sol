// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

interface IState {
    function claimRank(uint256 term) external;
}

/// @title Worker
/// @notice Per-claim contract deployed via CREATE2 by `Multicall`. Calls
/// `State.claimRank` once in its constructor; the deployed contract code
/// persists, producing one new account-trie entry per deployment.
contract Worker {
    constructor(address state, uint256 term) {
        IState(state).claimRank(term);
    }
}
