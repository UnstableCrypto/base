#!/usr/bin/env bash
# Rustc wrapper that replaces -C metadata with a deterministic hash
# to eliminate non-determinism from absolute paths in Cargo's metadata
# computation. Cargo includes the absolute path of path dependencies
# in -C metadata, which changes symbol mangling across checkout locations.
#
# This wrapper intercepts rustc invocations from Cargo and replaces
# the -C metadata=<hash> argument with one computed from deterministic
# inputs: the crate name, package version, features, and a canonical
# source path (CARGO_MANIFEST_DIR remapped via repo root).
#
# Usage: RUSTC_WRAPPER=tools/rustc-deterministic-metadata.sh cargo build ...
#
# The wrapper expects REPRO_REPO_ROOT to be set to the repo root path
# so it can normalize CARGO_MANIFEST_DIR to a repo-relative path.
set -euo pipefail

RUSTC="$1"
shift

# Pass through non-compilation invocations (rustc -vV, etc.)
has_metadata=false
for arg in "$@"; do
    case "$arg" in
        -Cmetadata=*|metadata=*) has_metadata=true; break ;;
    esac
done

# Also check for "-C metadata=..." as two separate args
if ! "$has_metadata"; then
    prev=""
    for arg in "$@"; do
        if [ "$prev" = "-C" ]; then
            case "$arg" in
                metadata=*) has_metadata=true; break ;;
            esac
        fi
        prev="$arg"
    done
fi

if ! "$has_metadata"; then
    exec "$RUSTC" "$@"
fi

# Compute a deterministic source identity from CARGO_MANIFEST_DIR.
# Replace the absolute repo root prefix with a fixed string so that
# different checkout locations produce the same hash input.
manifest_dir="${CARGO_MANIFEST_DIR:-}"
repo_root="${REPRO_REPO_ROOT:-}"
if [ -n "$repo_root" ] && [ -n "$manifest_dir" ]; then
    # Normalize the manifest dir relative to the repo root
    canonical_dir="${manifest_dir#"$repo_root"}"
    # If it didn't start with repo_root, keep original (e.g. registry crate)
    if [ "$canonical_dir" = "$manifest_dir" ]; then
        canonical_dir="$manifest_dir"
    fi
else
    canonical_dir="$manifest_dir"
fi

# Gather deterministic inputs for the metadata hash
crate_name="${CARGO_CRATE_NAME:-unknown}"
pkg_name="${CARGO_PKG_NAME:-$crate_name}"
pkg_version="${CARGO_PKG_VERSION:-0.0.0}"

# Collect features from --cfg 'feature="..."' args
features=""
for arg in "$@"; do
    case "$arg" in
        --cfg) ;;
        feature=*) features="${features}${arg};" ;;
    esac
done

# Collect the target triple
target=""
prev=""
for arg in "$@"; do
    if [ "$prev" = "--target" ]; then
        target="$arg"
        break
    fi
    prev="$arg"
done

# Compute deterministic metadata hash
hash_input="pkg=${pkg_name}|ver=${pkg_version}|crate=${crate_name}|target=${target}|dir=${canonical_dir}|feat=${features}"
new_metadata=$(printf '%s' "$hash_input" | shasum -a 256 | cut -c1-16)

# Replace -C metadata=... in the argument list
new_args=()
skip_next=false
for arg in "$@"; do
    if "$skip_next"; then
        skip_next=false
        # This is the value after -C, check if it's metadata=...
        case "$arg" in
            metadata=*)
                new_args+=("metadata=$new_metadata")
                ;;
            *)
                new_args+=("$arg")
                ;;
        esac
        continue
    fi

    case "$arg" in
        -Cmetadata=*)
            new_args+=("-Cmetadata=$new_metadata")
            ;;
        -C)
            new_args+=("$arg")
            skip_next=true
            ;;
        *)
            new_args+=("$arg")
            ;;
    esac
done

exec "$RUSTC" "${new_args[@]}"
