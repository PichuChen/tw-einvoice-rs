# Durable submission state

The `tw-einvoice-state` crate separates protocol progress from invoice-processing
disposition so inbound Ministry results can be applied safely and idempotently.

## Domain model

Transport state is monotonic:

```text
Created
  -> Prepared
  -> Uploaded
  -> Enqueued
  -> ProcessObserved
  -> SummaryObserved
```

Repeating the same state is a no-op. Delayed/stale events are ignored rather
than allowed to regress the record.

Business outcome is independent from transport state. `ProcessResult` only
records platform acknowledgement and diagnostics. It does not infer a terminal
accepted/rejected state from undocumented diagnostic-code semantics.

A validated `SummaryResult` is the terminal invoice-count authority and maps to
one of:

- all good;
- all failed;
- partial success.

Before a terminal summary is accepted, the state layer checks:

- all bucket counts are non-negative;
- total = good + failed;
- listed invoice counts match declared bucket counts when lists are present;
- total matches the submission's expected quantity.

Exact result replay is idempotent. A different terminal summary for an already
terminal message ID is treated as a conflict and must not overwrite history.

## Correlation

The primary key is the platform/common message ID, for example:

```text
4.1-F0401-YYYYMMDD-HHmmssSSS-UUID
```

`SummaryResult` may contain multiple message entries. The state layer selects
exactly one matching ID and rejects missing or duplicate entries.

## Persistence boundary

The crate exposes `SubmissionStore` rather than depending on PostgreSQL or an
ORM. The persistence model uses optimistic concurrency:

- `load(message_id)` returns `VersionedSubmission { revision, record }`;
- `compare_and_swap(None, record)` means create only if absent;
- `compare_and_swap(Some(revision), record)` updates only when the stored
  revision still matches;
- successful writes atomically advance the revision;
- revision mismatch returns `StoreWriteOutcome::Conflict`.

A PostgreSQL implementation can use a table shaped approximately as:

```text
message_id          text primary key
revision            bigint not null
expected_quantity   integer not null
transport_state     text not null
domain_payload      jsonb / normalized child tables
updated_at          timestamptz not null
```

The exact physical schema remains intentionally outside the domain crate. A
future adapter may choose JSONB snapshots, normalized process-result rows, or
event tables as long as it preserves compare-and-swap semantics and does not
silently overwrite conflicting terminal state.

## Operational rules

- Persist the prepared/signed submission before network I/O.
- Record upload/enqueue progress monotonically.
- Apply `ProcessResult` as diagnostics only.
- Apply only consistent `SummaryResult` terminal data.
- Treat exact replay as success/no-op.
- Surface conflicting terminal summaries for operator review.
- Never derive a terminal business outcome from transport success alone.
