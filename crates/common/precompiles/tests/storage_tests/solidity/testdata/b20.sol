// SPDX-License-Identifier: MIT OR Apache-2.0
pragma solidity ^0.8.0;

/// Test contract for B20 token storage layout.
/// Includes roles, metadata, and ERC20 storage.
contract B20 {
    // ========== RolesAuth Storage ==========

    /// Nested mapping for role assignments: user -> role -> hasRole
    mapping(address => mapping(bytes32 => bool)) public roles;

    /// Mapping of role to its admin role
    mapping(bytes32 => bytes32) public roleAdmins;

    // ========== Metadata Storage ==========

    string public name;
    string public symbol;
    string public currency;
    // Unused slot, kept for storage layout compatibility
    bytes32 public domainSeparator;
    uint64 public transferPolicyId;

    // ========== ERC20 Storage ==========

    uint256 public totalSupply;
    mapping(address => uint256) public balances;
    mapping(address => mapping(address => uint256)) public allowances;
    mapping(address => uint256) public permitNonces;
    bool public paused;
    uint256 public supplyCap;
    // Unused slot, kept for storage layout compatibility
    mapping(bytes32 => bool) public salts;
}
