# #055: Evolution Delete

## Problem

Evolution events are immutable records. Users can revert active evolutions (undoing
artifacts while keeping the audit record) or reject proposals, but there is no way
to permanently remove an evolution event from the database. Terminal-state events
(rejected, reverted, failed) accumulate forever.

## Solution

Add a **Delete** action that permanently removes an evolution event and its artifacts.
Delete is separate from Revert: revert keeps the record, delete removes it.

### Delete Rules

| Current Status | Delete Allowed? | Behavior |
|----------------|-----------------|----------|
| Proposed | Yes | Delete DB record |
| Rejected | Yes | Delete DB record |
| Failed | Yes | Delete DB record |
| Reverted | Yes | Delete DB record |
| Active | Yes | Revert artifacts first (via existing gRPC), then delete DB record |
| Approved | No | Blocked (mid-execution pipeline) |
| Executing | No | Blocked (mid-execution pipeline) |

### Key Decisions

- **Hard delete** -- no soft-delete status or "deleted" enum variant.
- **Frees deduplication slot** -- deleting a record allows the agent to re-propose
  the same (type, title) combination.
- **No batch delete** -- single-event deletion only.
- **No archive/export** before deletion.

### API

`DELETE /api/v1/evolution/{id}` (admin-only). Returns 204 No Content on success.

### Flow: Active Event Delete

1. Service reads event, validates status is deletable.
2. Service calls `RevertEvolution` gRPC (agent checks Active, cleans artifacts,
   sets status to Reverted).
3. Service hard-deletes the DB row.

If gRPC revert fails, the delete is aborted and the error propagated.

### Flow: Non-Active Event Delete

1. Service reads event, validates status is deletable.
2. Service hard-deletes the DB row.
