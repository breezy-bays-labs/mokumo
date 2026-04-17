#!/usr/bin/env bash
# R13 — action-string continuity.
#
# Pre-Stage-3 `activity_log.action` rows store un-prefixed verbs
# (`"created"`, `"updated"`, `"soft_deleted"`, `"restored"`). Stage 3 extracts
# the shop vertical into `crates/mokumo-shop/src/activity.rs` via an
# `ActivityAction::as_str()` arm per variant. Any drift in those string
# literals is an activity-log continuity break — in-flight shops' audit
# trails would stop matching the search/filter code reading them back.
#
# This script greps the arms as a low-cost CI guard. The unit tests in
# `crates/mokumo-shop/src/activity.rs` are the primary defence; this script
# catches the case where someone "cleans up" the enum without also updating
# the tests.

set -euo pipefail

TARGET="${TARGET:-crates/mokumo-shop/src/activity.rs}"

if [[ ! -f "$TARGET" ]]; then
    echo "::error::R13 script error: ${TARGET} does not exist" >&2
    exit 2
fi

fail=0

# Negative checks: no prefixed literals for shop-vertical entity kinds.
for forbidden in 'customer_created' 'customer_updated' 'customer_soft_deleted' 'customer_restored' \
                  'garment_created' 'garment_updated' 'garment_soft_deleted' 'garment_restored'; do
    if grep -qE "\"${forbidden}\"" "$TARGET"; then
        echo "::error::R13 violation: found forbidden prefixed action literal '${forbidden}' in ${TARGET}" >&2
        fail=1
    fi
done

# Positive checks: the four core verbs must appear as `=> "verb",` arms.
for required in 'created' 'updated' 'soft_deleted' 'restored'; do
    if ! grep -qE "=> \"${required}\"," "$TARGET"; then
        echo "::error::R13 violation: missing required action literal '${required}' in ${TARGET}" >&2
        fail=1
    fi
done

if [[ $fail -ne 0 ]]; then
    exit 1
fi

echo "R13 ok: shop-vertical action literals match pre-Stage-3 contract"
