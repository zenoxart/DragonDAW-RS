# Red Hive – architecture

## Principles

1. **Errors are values.** Every operation returns `Res<T> = Result<T, Error>`. An `Error` carries a kind
   (validation, not-found, forbidden, conflict, unavailable, …), a stable code, a *text key* for the UI,
   field errors and metadata. Over HTTP it becomes RFC 7807 problem details (`ApiError`); the client turns it
   back into the same `Error` and shows the localized text.
2. **Railway use cases.** Handlers follow *validate → authorize → load → decide → persist (+ outbox event in the
   same transaction) → project*. Validation collects *all* field errors (`Validator`).
3. **Contracts first.** `redhive-contracts` is shared by every service, the client and the CLI: DTOs,
   module manifests and their contract surface, view packs, the workflow model and expression language.
4. **The server drives the UI.** Modules never ship native client code. They ship a *view pack* –
   declarative dashboards, lists, forms and report runners bound to their own REST endpoints. That keeps the
   host safe and makes hot-swapping a module trivial.
5. **Everything is an event.** Changes write an outbox row in their transaction; a dispatcher posts it to the
   notification point which stores, fans out (SSE, WNS, subscribers such as the workflow service) and replays.

## Services

| Service | Responsibility | Main routes |
|---|---|---|
| api-gateway | single entry point, routing table from the registry, rate limit, correlation ids | `/*`, `/api/gateway/routes` |
| identity-service | sign-in, refresh, profile, password, avatar, roles/permissions, delegated tokens | `/api/identity/*` |
| configuration-service | scoped settings | `/api/config/*` |
| module-registry | catalog, assignment per user, versions, publish (contract diff + impact + rollout), enable/withdraw | `/api/modules/*` |
| module-delivery | package storage, ECDSA P-256 signing, download, signing key | `/api/packages/*` |
| update-orchestrator | staged rollouts Pilot → Broad → Complete / Held / Cancelled, version per user | `/api/updates/rollouts/*` |
| notification-point | event store, SSE stream with catch-up, channels, subscriber cursors, WNS | `/api/notifications/*` |
| workflow-service | definitions, instances, inbox, triggers, impact reports, templates, command events | `/api/workflows/*` |
| ai-agent | proposals for AI steps, guardrails, kill switch, audit | `/api/ai/*` |
| document-service | documents and invoices (submit/approve/reject/pay), stats | `/api/documents/*` |
| reporting-service | report catalogue, runs | `/api/reports/*` |
| user-service | user administration, roles, organization | `/api/users/*` |

All services share `redhive-backend-core`: options from the environment, PostgreSQL pool with retries,
token validation (`RequestContext` extractor), internal calls (`x-redhive-internal-key`), `ServiceClient`,
JSON extractors that answer with problem details, outbox writer and dispatcher, audit log, health endpoints
(`/health/live`, `/health/ready`).

## Module lifecycle

```
publish manifest ─▶ check_manifest (prefixes, host version, dependencies)
                 ─▶ diff_surfaces(old, new)   Compatible | Migratable | ReviewRequired | Breaking
                 ─▶ workflow impact (dry run) Compatible | AutoMigrated | ReviewRequired | Broken
                 ─▶ store version, apply automatic migrations (renames incl. expression rewriting)
                 ─▶ start rollout: Pilot ─▶ Broad ─▶ Complete     (Held if workflows would break)
                 ─▶ event modules.version.available ─▶ clients hot-swap
```

The client verifies every package: SHA-256 of the ZIP, signature over the hex digest with the server's
public key, pinned per server on first use (a changed key is refused until the user trusts it).

## Hot-swap in the client

`modules.version.available` (for this user) → wait until the module has no running actions (max. a few
seconds) → download and verify → unregister texts, icons, commands, views of the old version → register the
new ones → mark open views dirty. If anything fails the previous version stays registered.

## Workflows

A workflow is a JSON definition: input schema, trigger, steps, AI policy. Steps reference values with a
small, safe expression language (`vars.amount > 1000 && user.department == "Finance"`, `split`, `join`,
`if`, …) – parsed, validated and evaluated without any code execution. The engine persists every
instance; waits (user input, events, schedules, sub-workflows) survive restarts. Module steps are executed
with a delegated token of the user the workflow runs as. With an AI policy, the agent may propose or execute
steps; critical steps are turned into a confirmation task and then run *as the confirming person*.

When a module changes, the impact analysis checks every workflow that references its commands, step types,
input schemas or events; renames are migrated automatically, removed or newly required elements are reported
and AI triggers of broken workflows are suspended.

## Client

`redhive-client-framework` contains the services, `redhive-host` the window:

| Component | Purpose |
|---|---|
| `AppContext` | the composition root passed to every view: api, language, icons, commands, shortcuts, navigation, log, session, toasts |
| `CommandPool` | commands with placements (main menu, toolbar, profile, navigation, wheel), permission checks, fuzzy search |
| `ShortcutManager` | layered bindings (shell → module → preset → user), chords (`Ctrl+K, Ctrl+S`), scopes, conflicts, recording |
| `NavigationRegistry` + `NavigationWheel` | every view, MRU, the radial Ctrl+Tab switcher |
| `DockManager` / `WindowManager` | VS-style layout, named layouts, detached OS windows with monitor-aware placement |
| `NotificationHub` | SSE with reconnect and catch-up by sequence |
| `modules` / `HotSwapManager` | load, verify, register, swap |
| `views` | renders view packs (dashboards, lists, forms, report runner) with responsive size classes |
| `WorkflowsUi` | inbox, list, designer, runs, impact and AI panels, recorder |

Long-running work runs on a shared tokio runtime and is polled with `Promise<T>`; the UI never blocks.
