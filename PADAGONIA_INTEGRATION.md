# PADAGONIA Integration Roadmap

See `/home/sal/padagonia/docs/enterprise-integration-directives.md`.

## Modules

- `scan_event_adapter`: normalize directory/file scans, sizes, counts, and
  modification times.
- `temporal_writer`: persist bounded snapshots and scan hashes.
- `hotspot_reader`: identify growth, bloat, and recurring heavy paths.
- `cleanup_evidence`: record proposed and applied cleanup with operator and
  recovery metadata.

## Acceptance gates

Scans are idempotent, high-cardinality file paths are bounded, and cleanup is
never triggered by graph-derived advice alone.
