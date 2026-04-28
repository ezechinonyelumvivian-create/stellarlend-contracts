# Activity Feed Ordering and Pagination Guarantees

## Overview

`get_recent_activity` and `get_user_activity` return entries from a bounded
in-contract log (`ActivityLog`, max 10,000 entries). This document describes
the ordering contract, pagination semantics, and eviction behaviour that
indexers and UI consumers can rely on.

---

## Ordering

Entries are returned **newest-first** (reverse insertion order).

- Index `0` of the returned vector is always the most recently recorded entry.
- Timestamps are non-decreasing in insertion order, so the returned slice has
  non-increasing timestamps.
- Within a single ledger (same timestamp) the relative order of entries
  matches insertion order, reversed.

---

## Pagination

Both functions accept `limit: u32` and `offset: u32`.

| Condition | Result |
|---|---|
| `offset >= total` | Empty vector |
| `offset + limit > total` | Returns the remaining `total - offset` entries |
| `limit == 0` | Empty vector |
| `offset` very large (e.g. `u32::MAX/2`) | Empty vector, no panic |

Stable pagination: walking the log with consecutive `(limit, offset)` windows
covers every entry exactly once with no gaps and no overlaps, provided the log
is not modified between calls.

---

## Eviction

When the log reaches 10,000 entries the **oldest** entry (lowest insertion
index / lowest timestamp) is evicted via `pop_front` before the new entry is
appended. The log therefore always holds the most recent ≤ 10,000 entries.

---

## Per-User Feed

`get_user_activity` filters the global log by `Address` equality before
applying `limit`/`offset`. Each user's feed contains only their own entries;
no cross-user data leakage is possible.

---

## Event Schema

Activity entries carry an `activity_type: Symbol` field. Current values:

| Symbol | Emitted by |
|---|---|
| `"deposit"` | `deposit_collateral`, `deposit_collateral_asset` |
| `"borrow"` | `borrow`, `borrow_asset` |
| `"repay"` | `repay_debt`, `repay_asset` |
| `"withdraw"` | `withdraw`, `withdraw_asset`, `ca_withdraw_collateral` |
| `"liquidate"` | `liquidate` |

The `activity_type` symbol set is additive-only. Indexers should handle
unknown symbols gracefully rather than failing.

See `docs/EVENT_SCHEMA_VERSIONING.md` for the broader event versioning policy.

---

## Test Coverage

`src/tests/analytics_test.rs` contains deterministic tests for all guarantees
above:

| Test | Guarantee verified |
|---|---|
| `test_activity_ordering_newest_first_under_load` | Newest-first ordering |
| `test_activity_pagination_covers_full_log_under_load` | Full coverage, no gaps |
| `test_activity_pagination_no_overlap_between_pages` | No overlap between pages |
| `test_activity_log_eviction_at_capacity` | Cap enforced at 10,000 |
| `test_activity_log_eviction_drops_oldest_entry` | Oldest entry evicted first |
| `test_user_activity_feed_isolation_under_load` | Per-user isolation |
| `test_user_activity_feed_pagination_under_load` | User feed full coverage |
| `test_pagination_offset_equals_total_returns_empty` | Boundary: offset == total |
| `test_pagination_limit_larger_than_remaining_returns_remainder` | Partial last page |
| `test_pagination_zero_limit_returns_empty` | Zero limit |
| `test_pagination_large_offset_no_panic` | No overflow on large offset |
