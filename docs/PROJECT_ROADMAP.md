# N-xport Project Roadmap

This roadmap tracks work needed to make N-xport reliable for day-to-day N-central exports, imports, and migrations. Check items off as their acceptance criteria are met. Keep scope decisions visible here rather than treating unimplemented UI options as supported.

## Working agreements

- A task is complete when its acceptance criteria are met and relevant checks pass.
- Add regression tests for bugs and API compatibility fixes.
- Validate behavior against the checked-in OpenAPI spec and, where practical, a representative N-central instance.
- Keep unsupported operations disabled and clearly labeled until they are implemented end-to-end.
- Update this roadmap when priorities or scope decisions change.

## Phase 0 — Stabilize current OpenAPI alignment

The fixes below are implemented and have passed the Rust library tests and frontend production build.

- [x] Parse documented pagination fields (`pageNumber`, `pageSize`, `totalItems`, and `totalPages`) without requiring every server to return the same metadata.
- [x] Fetch service organizations across pages.
- [x] Accept `propertyName` as the custom-property label.
- [x] Map the documented processor count into the logical-processor export column.
- [x] Parse categorized device asset responses while retaining legacy response compatibility.
- [x] Use the documented organization custom-property PUT endpoint before SOAP fallback.
- [x] Show a SOAP prerequisite notice for import and migration selections that may call SOAP.
- [x] Add targeted Rust regression tests; `cargo test --lib` passes.
- [x] Verify the frontend production build and `git diff --check`.
- [x] Commit the OpenAPI alignment work and the spec file together.
- [ ] Verify the changed request/response behavior against at least one representative N-central server.

## Phase 1 — Correct operation outcomes

### Cancellation

- [x] Replace the unused shared cancellation flag with a cancellation mechanism observed by export, import, and migration work.
- [x] Check cancellation between phases, per-device fetches, and per-row import/migration operations; avoid starting additional work after cancellation is requested.
- [x] Return and display an explicit cancelled outcome distinct from success and failure.
- [ ] Add tests proving cancellation stops each operation and does not report it as completed successfully.

**Acceptance:** pressing Cancel stops further work promptly at a safe boundary, leaves already-written output intact, and shows a cancelled result.

### Migration result accuracy

- [x] Track customer, site, property, fetch, and other per-item failures in the migration summary.
- [x] Include every failure category in final issue/success calculation.
- [x] Return a partial/failure result when selected work fails; return issue, skipped, and manual-follow-up details to the UI.
- [x] Make skipped, already-existing, and not-updated outcomes explicit and separate from failures.
- [x] Add tests for all-success, partial-failure, cancellation, and all-failure summaries.

**Acceptance:** the final result cannot say “completed successfully” when a selected operation failed, and the summary identifies what succeeded, failed, or needs manual follow-up.

### Export record counts

- [x] Define the reported total as logical records written across selected resources, not total rows across output formats.
- [x] Count each logical record once for CSV, JSON, and dual-format exports, including exports with a separate CSV shape.
- [x] Add tests asserting counts for CSV-only, JSON-only, dual-format, and split-shape dual-format exports.

**Acceptance:** with CSV and JSON both selected, the completion message and result count communicate the agreed count unambiguously.

## Phase 2 — API and data correctness

- [x] Add a mocked API test suite for authentication, request/response parsing, pagination, rate limits, error responses, and the documented custom-property update request.
- [x] Test pagination with only `totalItems`, only `totalPages`, both fields, no metadata, an empty page, and a final partial page.
- [x] Verify supported API models and create/update payloads against the OpenAPI spec; document intentional differences or server-specific compatibility behavior.
- [x] Add representative fixtures for users, roles, access groups, properties, devices, and assets.
- [x] Check that exports preserve useful server fields and do not silently lose data needed for migration.
- [x] Surface per-item fetch warnings in the final export result and distinguish a complete export from a partial export.
- [x] Verify scope filtering for sites and devices across SO, customer, and site hierarchy levels.

**Acceptance:** core API behavior is testable without a live tenant, spec changes can be checked against fixtures, and partial export coverage is apparent to the user.

## Phase 3 — Decide and deliver supported feature scope

### Scope decisions

- [ ] Decide whether service-org import is needed; otherwise keep it explicitly unsupported.
- [ ] Decide whether device import/migration is in scope, including device creation, activation, field mapping constraints, and device-based access-group mapping; device-group rows are currently rejected/reported for manual follow-up.
- [ ] Decide whether organization-property import is in scope and whether migration support should be enabled in the UI.
- [ ] Decide whether device-property and device-asset import/migration are in scope.
- [ ] Record the decisions and supported N-central version assumptions here and in the user documentation.

### End-to-end implementation

- [ ] For each approved resource, implement validation, dry-run planning, live execution, useful per-row outcomes, and tests.
- [ ] Ensure dependent resource ordering and ID mapping are explicit and tested.
- [ ] Keep unimplemented resource types disabled in the UI and reject them in backend commands.
- [ ] Test idempotency and duplicate handling for repeated imports/migrations.

**Acceptance:** every advertised operation is implemented end-to-end, tested, and accurately described; everything else remains visibly unsupported.

## Phase 4 — Migration safety and fidelity

- [ ] Review migration ordering and dependencies for customers/sites, roles, users, access groups, and properties.
- [ ] Validate source-to-destination mappings for names, IDs, org-unit scopes, and inherited entities.
- [ ] Define and test behavior when an entity with the same name exists at a different org-unit scope.
- [ ] Make existing access-group membership limitations clear before migration and in the result summary.
- [ ] Confirm SOAP is only attempted where required or intentionally configured; test operation behavior when SOAP is disabled.
- [ ] Add a preflight summary of selected resources, source/destination SOs, expected SOAP use, and known manual follow-up.

**Acceptance:** a dry run gives an actionable preview, live execution preserves supported relationships, and limitations or manual work are not hidden.

## Phase 5 — Usability, security, and release readiness

- [ ] Clearly warn when credential storage falls back to obfuscation instead of the OS keychain; consider removing the insecure fallback if product requirements allow.
- [ ] Review credential deletion and retrieval paths for consistent keychain and fallback behavior.
- [ ] Validate profile switching and reconnect behavior, including source/destination state cleanup.
- [ ] Add frontend tests for critical flows: connection, import dry-run/apply, export selection, SOAP notice, and cancellation.
- [ ] Add CI checks for Rust formatting, Rust tests, Clippy (with an agreed warning policy), TypeScript checks, and frontend build.
- [ ] Reconcile README and technical overview with actual feature support, API version, SOAP requirements, and platform requirements.
- [ ] Test packaged release artifacts and updater behavior on supported operating systems.

**Acceptance:** CI covers the primary checks, user-facing docs match the shipped behavior, and release smoke tests cover supported platforms.

## Known review findings to track

- Cancellation is now observed at safe work boundaries and returns an explicit outcome. An in-flight request is allowed to finish; operation-level cancellation tests remain open above.
- Migration failures, skipped items, and manual follow-up are now separated in the result and affect success status.
- Export totals now count each logical record once, even when CSV and JSON are both selected.
- `cargo clippy --all-targets --all-features` currently reports warnings; decide which warnings must be eliminated or explicitly allowed in CI.
- The API client now has local mock-server coverage; representative live-server validation and end-to-end workflow coverage remain open.
