# Writing a Red Hive module

```
modules/my-module/
  manifest.json        identity, prefix, permissions, dependencies, backend route, contract surface
  client/views.json    view pack: texts, icons, menu groups, views, commands
  backend/             optional axum service (uses redhive-backend-core)
  workflows/*.json     optional workflow templates
```

## manifest.json

```json
{
  "moduleId": "my-module",
  "version": "1.0.0",
  "displayName": "my:module.name",
  "icon": "my.home",
  "prefix": "my",
  "minHostVersion": "1.0.0",
  "client": { "package": "my-module-1.0.0.zip", "viewPack": "views.json", "hotSwap": "supported" },
  "backend": { "service": "http://my-service:8080", "apiVersion": "v1", "route": "/api/my" },
  "permissions": ["my.view", "my.edit"],
  "dependencies": [{ "moduleId": "user-management", "versionRange": ">=1.0.0 <2.0.0" }],
  "languages": ["en", "de"],
  "contractSurface": {
    "backendApiVersion": "v1",
    "commands": [], "inputSchemas": [], "stepTypes": [], "events": []
  }
}
```

Everything the module owns starts with its **prefix**: text keys (`my:view.home`), icons (`my.home`),
commands, permissions (`my.view`) and workflow step types (`my.createThing`). `check_manifest` enforces it.

The full real-world examples are `modules/document-management`, `modules/reporting` and
`modules/user-management`.

The **contract surface** lists what workflows may depend on. When you publish a new version it is diffed
against the current one: adding things is compatible; renames declared in `migrations` are migrated
automatically; removing something or adding a required field puts dependent workflows into review or marks
them broken and holds the rollout.

## client/views.json

```json
{
  "moduleId": "my-module", "version": "1.0.0",
  "texts": { "en": { "module.name": "My module", "view.home": "Overview" },
             "de": { "module.name": "Mein Modul", "view.home": "Übersicht" } },
  "icons": { "home": "📁" },
  "groups": [{ "menu": "shell.mainmenu", "path": "my", "label": "my:module.name", "order": 30 }],
  "views": [{
    "key": "my.home", "title": "my:view.home", "icon": "my.home", "home": true, "permission": "my.view",
    "liveEvents": ["my.*"],
    "body": {
      "kind": "list", "endpoint": "/api/my/things", "items": "items", "idField": "id", "search": true,
      "columns": [{ "field": "name", "label": "my:col.name" }, { "field": "amount", "label": "my:col.amount", "format": "money" }],
      "filters": [{ "param": "status", "label": "my:filter.status", "options": [{ "value": "open", "label": "my:status.open" }] }],
      "toolbarActions": [{ "id": "add", "label": "my:action.add", "icon": "add", "permission": "my.edit",
                           "method": "post", "endpoint": "/api/my/things",
                           "form": { "schemaId": "my.thing", "fields": [{ "name": "name", "label": "my:col.name", "type": "text", "required": true }] } }],
      "rowActions": [{ "id": "delete", "label": "my:action.delete", "method": "delete",
                       "endpoint": "/api/my/things/{id}", "confirm": "my:confirm.delete" }]
    }
  }],
  "commands": []
}
```

View kinds: `dashboard` (stat cards, chart, lists, actions), `list` (columns, filters, grouping, detail pane,
row/toolbar actions), `form` (load + submit) and `reportRunner`. Actions open a form built from an
`InputSchema` (text, number, money, date, choice, lookup from an endpoint, …), call the endpoint and refresh
the view; they can be bound to shortcuts and are recorded by the workflow recorder.

Icons are glyphs of the bundled fonts (`cargo test` checks that every icon can be drawn).

## Backend

A module backend is an ordinary axum service built on `redhive-backend-core`:

```rust
#[tokio::main]
async fn main() {
    let core = Core::init("my-service").await;
    let router = Router::new().route("/api/my/things", get(list).post(create));
    serve(core, router, ()).await
}
```

Use `RequestContext` for the signed-in user (`ctx.user.require("my.edit")?`), `ReqJson<T>` for bodies,
`OutboxWriter` to publish events in the same transaction, and return `ApiResult<T>`.
To offer workflow steps, expose `GET /api/my/workflow-steps` and `POST /api/my/workflow-steps/{type}`
and declare the step types in the contract surface.

## Publishing

```bash
redhive-cli pack    modules/my-module                 # artifacts/my-module-1.0.0.zip
redhive-cli publish modules/my-module --server URL --token TOKEN [--immediate]
```

During development the registry and delivery services seed every module found in `REDHIVE__MODULES_DIR`
at startup, and the client can load view packs straight from disk with `REDHIVE_DEV_MODULES=<modules dir>`.
