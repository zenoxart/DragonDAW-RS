//! Modules: manifest, contract surface (+ diff), compatibility, dependency resolution, registry DTOs.

use crate::workflows::input::{FieldType, InputSchema};
use chrono::{DateTime, Utc};
use redhive_monads::{Error, Res};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

/// Version of the module contracts this framework speaks.
pub const CONTRACTS_VERSION: u32 = 1;

/// The module manifest: identity, client view pack, backend service, permissions and contract surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleManifest {
    pub module_id: String,
    pub version: String,
    /// Text key of the display name (`documents:module.name`).
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub accent_color: Option<String>,
    #[serde(default = "default_host_version")]
    pub min_host_version: String,
    #[serde(default = "default_contracts")]
    pub contracts_version: u32,
    /// Prefix of text keys, commands, icons and permissions.
    pub prefix: String,
    pub client: ClientPackage,
    #[serde(default)]
    pub backend: Option<BackendInfo>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<ModuleDependency>,
    #[serde(default = "default_languages")]
    pub languages: Vec<String>,
    #[serde(default = "default_language")]
    pub default_language: String,
    #[serde(default)]
    pub contract_surface: ContractSurface,
}

fn default_host_version() -> String {
    "1.0.0".into()
}
fn default_contracts() -> u32 {
    CONTRACTS_VERSION
}
fn default_languages() -> Vec<String> {
    vec!["en".into()]
}
fn default_language() -> String {
    "en".into()
}

/// How the client part is delivered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientPackage {
    /// File name of the signed package (`document-management-1.0.0.zip`).
    #[serde(default)]
    pub package: Option<String>,
    /// View pack inside the package.
    #[serde(default = "default_view_pack")]
    pub view_pack: String,
    #[serde(default)]
    pub hot_swap: HotSwapMode,
}

fn default_view_pack() -> String {
    "views.json".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum HotSwapMode {
    /// Swapped into the running client.
    #[default]
    Supported,
    /// The user is asked to restart the client.
    RestartRequired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    /// Base URL of the backend container (`http://document-service:8080`).
    pub service: String,
    #[serde(default = "default_api_version")]
    pub api_version: String,
    /// Public route prefix behind the gateway (`/api/documents`).
    pub route: String,
}

fn default_api_version() -> String {
    "v1".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleDependency {
    pub module_id: String,
    /// `>=1.0.0 <3.0.0`, `^1.2`, `~1.4.0`.
    pub version_range: String,
    #[serde(default)]
    pub optional: bool,
}

/// Everything workflows may depend on. Diffed on every publish.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContractSurface {
    #[serde(default = "default_api_version")]
    pub backend_api_version: String,
    #[serde(default)]
    pub commands: Vec<CommandContract>,
    #[serde(default)]
    pub input_schemas: Vec<InputSchema>,
    #[serde(default)]
    pub step_types: Vec<StepTypeContract>,
    #[serde(default)]
    pub events: Vec<EventContract>,
    /// Workflow template ids shipped with the module.
    #[serde(default)]
    pub templates: Vec<String>,
    #[serde(default)]
    pub migrations: Vec<ContractMigration>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandContract {
    pub id: String,
    #[serde(default = "one")]
    pub parameter_schema_version: u32,
    #[serde(default)]
    pub workflow_capable: bool,
    #[serde(default)]
    pub ai_allowed: bool,
    /// Headless endpoint the engine calls when the command runs inside a workflow.
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub input: Option<InputSchema>,
}

fn one() -> u32 {
    1
}

/// A headless operation workflows can run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepTypeContract {
    pub id: String,
    #[serde(default = "one")]
    pub version: u32,
    pub title: String,
    pub input: InputSchema,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default = "yes")]
    pub headless: bool,
    pub endpoint: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventContract {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default = "one")]
    pub payload_version: u32,
    /// Payload fields filters and mappings may read.
    #[serde(default)]
    pub fields: Vec<String>,
}

/// A declared rename that lets dependent workflows migrate automatically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractMigration {
    pub kind: ElementKind,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub field_renames: BTreeMap<String, String>,
    #[serde(default)]
    pub output_renames: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ElementKind {
    Command,
    InputSchema,
    StepType,
    Event,
}

/// How bad a change is for dependent workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeSeverity {
    /// Additive / no effect.
    Compatible,
    /// A declared migration rewrites dependent workflows.
    Migratable,
    /// Dependent workflows must be reviewed by a person.
    ReviewRequired,
    /// Dependent workflows can no longer run.
    Breaking,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "change")]
pub enum ChangeKind {
    Added,
    Removed,
    Renamed { to: String },
    Changed,
}

/// One difference between two contract surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceChange {
    pub element: ElementKind,
    pub id: String,
    #[serde(flatten)]
    pub kind: ChangeKind,
    pub severity: ChangeSeverity,
    #[serde(default)]
    pub details: Vec<String>,
    /// Field renames to apply (from migrations).
    #[serde(default)]
    pub field_renames: BTreeMap<String, String>,
    #[serde(default)]
    pub output_renames: BTreeMap<String, String>,
    /// Fields that became required / were removed / changed type (used to check which workflows are affected).
    #[serde(default)]
    pub affected_fields: Vec<String>,
}

impl ContractSurface {
    fn migration(&self, kind: ElementKind, from: &str) -> Option<&ContractMigration> {
        self.migrations.iter().find(|m| m.kind == kind && m.from == from)
    }
}

/// Compares the previous and the new surface of a module.
pub fn diff_surfaces(old: &ContractSurface, new: &ContractSurface) -> Vec<SurfaceChange> {
    let mut changes = Vec::new();

    // ---- step types
    let new_steps: HashMap<&str, &StepTypeContract> = new.step_types.iter().map(|s| (s.id.as_str(), s)).collect();
    for old_step in &old.step_types {
        match new_steps.get(old_step.id.as_str()) {
            Some(n) => {
                let in_place = new.migration(ElementKind::StepType, &old_step.id).filter(|m| m.to == old_step.id);
                let renames = in_place.map(|m| m.field_renames.clone()).unwrap_or_default();
                let mut c = compare_schemas(ElementKind::StepType, &old_step.id, &old_step.input, &n.input, &renames);
                if let Some(m) = in_place {
                    c.field_renames = m.field_renames.clone();
                    c.output_renames = m.output_renames.clone();
                    if !m.output_renames.is_empty() {
                        c.severity = c.severity.max(ChangeSeverity::Migratable);
                    }
                }
                let removed_outputs: Vec<_> = old_step.outputs.iter().filter(|o| !n.outputs.contains(o) && !c.output_renames.contains_key(*o)).cloned().collect();
                if !removed_outputs.is_empty() {
                    c.details.push(format!("outputs removed: {}", removed_outputs.join(", ")));
                    c.severity = c.severity.max(ChangeSeverity::ReviewRequired);
                    c.affected_fields.extend(removed_outputs.iter().map(|o| format!("output:{o}")));
                }
                if c.severity > ChangeSeverity::Compatible || n.version != old_step.version {
                    if n.version != old_step.version {
                        c.details.push(format!("version {} → {}", old_step.version, n.version));
                    }
                    changes.push(c);
                }
            }
            None => match new.migration(ElementKind::StepType, &old_step.id) {
                Some(m) if new_steps.contains_key(m.to.as_str()) => {
                    let target = new_steps[m.to.as_str()];
                    let mut c = compare_schemas(ElementKind::StepType, &old_step.id, &old_step.input, &target.input, &m.field_renames);
                    c.kind = ChangeKind::Renamed { to: m.to.clone() };
                    c.severity = c.severity.max(ChangeSeverity::Migratable);
                    c.field_renames = m.field_renames.clone();
                    c.output_renames = m.output_renames.clone();
                    changes.push(c);
                }
                _ => changes.push(SurfaceChange::removed(ElementKind::StepType, &old_step.id)),
            },
        }
    }
    let renamed_targets: BTreeSet<&str> = new.migrations.iter().filter(|m| m.from != m.to).map(|m| m.to.as_str()).collect();
    for s in &new.step_types {
        if !old.step_types.iter().any(|o| o.id == s.id) && !renamed_targets.contains(s.id.as_str()) {
            changes.push(SurfaceChange::added(ElementKind::StepType, &s.id));
        }
    }

    // ---- input schemas
    let new_schemas: HashMap<&str, &InputSchema> = new.input_schemas.iter().map(|s| (s.schema_id.as_str(), s)).collect();
    for old_schema in &old.input_schemas {
        match new_schemas.get(old_schema.schema_id.as_str()) {
            Some(n) => {
                let in_place = new.migration(ElementKind::InputSchema, &old_schema.schema_id).filter(|m| m.to == old_schema.schema_id);
                let renames = in_place.map(|m| m.field_renames.clone()).unwrap_or_default();
                let mut c = compare_schemas(ElementKind::InputSchema, &old_schema.schema_id, old_schema, n, &renames);
                c.field_renames = renames;
                if c.severity > ChangeSeverity::Compatible {
                    changes.push(c);
                }
            }
            None => match new.migration(ElementKind::InputSchema, &old_schema.schema_id) {
                Some(m) if new_schemas.contains_key(m.to.as_str()) => {
                    let mut c = compare_schemas(ElementKind::InputSchema, &old_schema.schema_id, old_schema, new_schemas[m.to.as_str()], &m.field_renames);
                    c.kind = ChangeKind::Renamed { to: m.to.clone() };
                    c.severity = c.severity.max(ChangeSeverity::Migratable);
                    c.field_renames = m.field_renames.clone();
                    changes.push(c);
                }
                _ => changes.push(SurfaceChange::removed(ElementKind::InputSchema, &old_schema.schema_id)),
            },
        }
    }

    // ---- commands
    for old_cmd in &old.commands {
        match new.commands.iter().find(|c| c.id == old_cmd.id) {
            Some(n) => {
                let mut c = SurfaceChange::changed(ElementKind::Command, &old_cmd.id);
                if old_cmd.workflow_capable && !n.workflow_capable {
                    c.severity = ChangeSeverity::Breaking;
                    c.details.push("no longer workflow-capable".into());
                }
                if old_cmd.ai_allowed && !n.ai_allowed {
                    c.severity = c.severity.max(ChangeSeverity::ReviewRequired);
                    c.details.push("no longer allowed for the AI agent".into());
                    c.affected_fields.push("aiAllowed".into());
                }
                if n.parameter_schema_version != old_cmd.parameter_schema_version {
                    c.severity = c.severity.max(ChangeSeverity::ReviewRequired);
                    c.details.push(format!("parameter schema {} → {}", old_cmd.parameter_schema_version, n.parameter_schema_version));
                }
                if c.severity > ChangeSeverity::Compatible {
                    changes.push(c);
                }
            }
            None => match new.migration(ElementKind::Command, &old_cmd.id) {
                Some(m) => {
                    let mut c = SurfaceChange::changed(ElementKind::Command, &old_cmd.id);
                    c.kind = ChangeKind::Renamed { to: m.to.clone() };
                    c.severity = ChangeSeverity::Migratable;
                    changes.push(c);
                }
                None => changes.push(SurfaceChange::removed(ElementKind::Command, &old_cmd.id)),
            },
        }
    }

    // ---- events
    for old_ev in &old.events {
        match new.events.iter().find(|e| e.event_type == old_ev.event_type) {
            Some(n) if n.payload_version != old_ev.payload_version => {
                let mut c = SurfaceChange::changed(ElementKind::Event, &old_ev.event_type);
                c.severity = ChangeSeverity::ReviewRequired;
                c.details.push(format!("payload version {} → {}", old_ev.payload_version, n.payload_version));
                let removed: Vec<_> = old_ev.fields.iter().filter(|f| !n.fields.contains(f)).cloned().collect();
                c.affected_fields = removed;
                changes.push(c);
            }
            Some(_) => {}
            None => match new.migration(ElementKind::Event, &old_ev.event_type) {
                Some(m) => {
                    let mut c = SurfaceChange::changed(ElementKind::Event, &old_ev.event_type);
                    c.kind = ChangeKind::Renamed { to: m.to.clone() };
                    c.severity = ChangeSeverity::Migratable;
                    c.field_renames = m.field_renames.clone();
                    changes.push(c);
                }
                None => changes.push(SurfaceChange::removed(ElementKind::Event, &old_ev.event_type)),
            },
        }
    }
    for e in &new.events {
        if !old.events.iter().any(|o| o.event_type == e.event_type) && !renamed_targets.contains(e.event_type.as_str()) {
            changes.push(SurfaceChange::added(ElementKind::Event, &e.event_type));
        }
    }
    changes
}

fn compare_schemas(element: ElementKind, id: &str, old: &InputSchema, new: &InputSchema, renames: &BTreeMap<String, String>) -> SurfaceChange {
    let mut c = SurfaceChange::changed(element, id);
    for f in &old.fields {
        let new_name = renames.get(&f.name).cloned().unwrap_or_else(|| f.name.clone());
        match new.fields.iter().find(|n| n.name == new_name) {
            None => {
                c.severity = c.severity.max(ChangeSeverity::ReviewRequired);
                c.details.push(format!("field '{}' removed", f.name));
                c.affected_fields.push(f.name.clone());
            }
            Some(n) => {
                if n.field_type != f.field_type && !(n.field_type == FieldType::MultiLine && f.field_type == FieldType::Text) {
                    c.severity = c.severity.max(ChangeSeverity::ReviewRequired);
                    c.details.push(format!("field '{}' changed type {:?} → {:?}", f.name, f.field_type, n.field_type));
                    c.affected_fields.push(f.name.clone());
                }
                if n.required && !f.required && n.default.is_none() {
                    c.severity = c.severity.max(ChangeSeverity::ReviewRequired);
                    c.details.push(format!("field '{}' became required", f.name));
                    c.affected_fields.push(f.name.clone());
                }
                if new_name != f.name {
                    c.severity = c.severity.max(ChangeSeverity::Migratable);
                    c.details.push(format!("field '{}' renamed to '{}'", f.name, new_name));
                }
            }
        }
    }
    let old_names: BTreeSet<String> = old.fields.iter().map(|f| renames.get(&f.name).cloned().unwrap_or_else(|| f.name.clone())).collect();
    for n in &new.fields {
        if !old_names.contains(&n.name) {
            if n.required && n.default.is_none() {
                c.severity = c.severity.max(ChangeSeverity::ReviewRequired);
                c.details.push(format!("new required field '{}'", n.name));
                c.affected_fields.push(format!("+{}", n.name));
            } else {
                c.details.push(format!("new optional field '{}'", n.name));
            }
        }
    }
    c
}

impl SurfaceChange {
    fn changed(element: ElementKind, id: &str) -> Self {
        SurfaceChange {
            element,
            id: id.to_string(),
            kind: ChangeKind::Changed,
            severity: ChangeSeverity::Compatible,
            details: Vec::new(),
            field_renames: BTreeMap::new(),
            output_renames: BTreeMap::new(),
            affected_fields: Vec::new(),
        }
    }
    fn removed(element: ElementKind, id: &str) -> Self {
        let mut c = Self::changed(element, id);
        c.kind = ChangeKind::Removed;
        c.severity = ChangeSeverity::Breaking;
        c.details.push("removed without a migration".into());
        c
    }
    fn added(element: ElementKind, id: &str) -> Self {
        let mut c = Self::changed(element, id);
        c.kind = ChangeKind::Added;
        c
    }
}

// ------------------------------------------------------------------ versions and compatibility

/// Parses a version range in npm/NuGet style (`>=1.0.0 <3.0.0`) or Cargo style (`^1.2`).
pub fn parse_range(range: &str) -> Res<VersionReq> {
    let trimmed = range.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return Ok(VersionReq::STAR);
    }
    // Insert commas between space-separated comparators: ">=1.0.0 <3.0.0" → ">=1.0.0, <3.0.0".
    let mut parts: Vec<String> = Vec::new();
    for token in trimmed.split_whitespace() {
        if token.starts_with(|c: char| c.is_ascii_digit()) && parts.last().is_some_and(|p| p.chars().all(|c| "<>=~^".contains(c))) {
            let last = parts.pop().unwrap_or_default();
            parts.push(format!("{last}{token}"));
        } else {
            parts.push(token.trim_end_matches(',').to_string());
        }
    }
    VersionReq::parse(&parts.join(", "))
        .map_err(|e| Error::validation("modules.versionRange.invalid", "modules:versionRange.invalid").with_message(format!("{range}: {e}")))
}

pub fn parse_version(version: &str) -> Res<Version> {
    Version::parse(version.trim()).map_err(|e| Error::validation("modules.version.invalid", "modules:version.invalid").with_message(format!("{version}: {e}")))
}

/// Checks a manifest against the host: host version, contracts version, well-formed fields.
pub fn check_manifest(manifest: &ModuleManifest, host_version: &str) -> Res<()> {
    let v = parse_version(&manifest.version)?;
    let _ = v;
    if manifest.module_id.is_empty() || !manifest.module_id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(Error::validation("modules.id.invalid", "modules:id.invalid").with_metadata("moduleId", manifest.module_id.clone()));
    }
    if manifest.prefix.is_empty() {
        return Err(Error::validation("modules.prefix.missing", "modules:prefix.missing"));
    }
    if manifest.contracts_version != CONTRACTS_VERSION {
        return Err(Error::conflict("modules.contracts.unsupported", "modules:contracts.unsupported")
            .with_metadata("contractsVersion", manifest.contracts_version)
            .with_metadata("supported", CONTRACTS_VERSION));
    }
    let min = parse_version(&manifest.min_host_version)?;
    let host = parse_version(host_version)?;
    if host < min {
        return Err(Error::conflict("modules.host.tooOld", "modules:host.tooOld")
            .with_metadata("required", manifest.min_host_version.clone())
            .with_metadata("host", host_version.to_string()));
    }
    for p in &manifest.permissions {
        if !p.starts_with(&format!("{}.", manifest.prefix)) {
            return Err(Error::validation("modules.permission.prefix", "modules:permission.prefix").with_metadata("permission", p.clone()));
        }
    }
    for d in &manifest.dependencies {
        parse_range(&d.version_range)?;
    }
    for s in &manifest.contract_surface.step_types {
        if !s.id.starts_with(&format!("{}.", manifest.prefix)) {
            return Err(Error::validation("modules.stepType.prefix", "modules:stepType.prefix").with_metadata("stepType", s.id.clone()));
        }
    }
    Ok(())
}

/// Orders modules so that dependencies load first; drops modules whose required dependencies are missing
/// or out of range. Returns (load order, rejected with reason).
pub fn resolve_load_order(manifests: &[ModuleManifest]) -> (Vec<ModuleManifest>, Vec<(String, Error)>) {
    let by_id: HashMap<&str, &ModuleManifest> = manifests.iter().map(|m| (m.module_id.as_str(), m)).collect();
    let mut rejected: Vec<(String, Error)> = Vec::new();
    let mut ok: BTreeSet<String> = BTreeSet::new();

    // Iterate until stable: a module is OK when all its required dependencies are OK and in range.
    let mut changed = true;
    let mut candidates: BTreeSet<String> = manifests.iter().map(|m| m.module_id.clone()).collect();
    while changed {
        changed = false;
        for id in candidates.clone() {
            let m = by_id[id.as_str()];
            let mut reason = None;
            for d in m.dependencies.iter().filter(|d| !d.optional) {
                match by_id.get(d.module_id.as_str()) {
                    None => reason = Some(Error::conflict("modules.dependency.missing", "modules:dependency.missing").with_metadata("dependency", d.module_id.clone())),
                    Some(dep) => {
                        let in_range = parse_range(&d.version_range).ok().zip(parse_version(&dep.version).ok()).is_some_and(|(r, v)| r.matches(&v));
                        if !in_range {
                            reason = Some(
                                Error::conflict("modules.dependency.version", "modules:dependency.version")
                                    .with_metadata("dependency", d.module_id.clone())
                                    .with_metadata("range", d.version_range.clone())
                                    .with_metadata("found", dep.version.clone()),
                            );
                        } else if rejected.iter().any(|(r, _)| r == &d.module_id) {
                            reason = Some(Error::conflict("modules.dependency.rejected", "modules:dependency.rejected").with_metadata("dependency", d.module_id.clone()));
                        }
                    }
                }
            }
            if let Some(e) = reason {
                rejected.push((id.clone(), e));
                candidates.remove(&id);
                changed = true;
            }
        }
    }
    ok.extend(candidates);

    // Topological order (Kahn), cycles are rejected.
    let mut order = Vec::new();
    let mut placed: BTreeSet<String> = BTreeSet::new();
    loop {
        let next: Vec<String> = ok
            .iter()
            .filter(|id| !placed.contains(*id))
            .filter(|id| by_id[id.as_str()].dependencies.iter().all(|d| placed.contains(&d.module_id) || !ok.contains(&d.module_id)))
            .cloned()
            .collect();
        if next.is_empty() {
            break;
        }
        for id in next {
            placed.insert(id.clone());
            order.push(by_id[id.as_str()].clone());
        }
    }
    for id in ok.iter().filter(|id| !placed.contains(*id)) {
        rejected.push((id.clone(), Error::conflict("modules.dependency.cycle", "modules:dependency.cycle")));
    }
    (order, rejected)
}

// ------------------------------------------------------------------ registry / delivery / updates DTOs

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleVersionInfo {
    pub module_id: String,
    pub version: String,
    pub channel: String,
    pub status: String,
    pub published_at: DateTime<Utc>,
    #[serde(default)]
    pub published_by: Option<String>,
    pub manifest: ModuleManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishRequest {
    pub manifest: ModuleManifest,
    #[serde(default = "stable")]
    pub channel: String,
    /// Skip the rollout and make the version available to everyone immediately.
    #[serde(default)]
    pub immediate: bool,
}

fn stable() -> String {
    "stable".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub module_id: String,
    pub version: String,
    #[serde(default)]
    pub previous_version: Option<String>,
    pub changes: Vec<SurfaceChange>,
    #[serde(default)]
    pub impact_report_id: Option<Uuid>,
    #[serde(default)]
    pub rollout_id: Option<Uuid>,
    pub broken_workflows: i32,
    #[serde(default)]
    pub rollout_stage: Option<RolloutStage>,
}

/// A module as assigned to the signed-in user (registry → client).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssignedModule {
    pub manifest: ModuleManifest,
    #[serde(default)]
    pub package: Option<PackageInfo>,
    /// Newer version that is being rolled out to other users (informational).
    #[serde(default)]
    pub newer_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    pub module_id: String,
    pub version: String,
    pub file_name: String,
    pub sha256: String,
    pub size_bytes: i64,
    /// Base64 DER ECDSA P-256 signature over the SHA-256 hex digest.
    pub signature: String,
    /// Relative download URL through the gateway.
    pub download_url: String,
    pub uploaded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SigningKeyInfo {
    /// PEM (SPKI) public key the client uses to verify package signatures.
    pub public_key_pem: String,
    pub algorithm: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RolloutStage {
    Pilot,
    Broad,
    Complete,
    Held,
    Cancelled,
}

impl RolloutStage {
    pub fn as_str(self) -> &'static str {
        match self {
            RolloutStage::Pilot => "Pilot",
            RolloutStage::Broad => "Broad",
            RolloutStage::Complete => "Complete",
            RolloutStage::Held => "Held",
            RolloutStage::Cancelled => "Cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "Pilot" => RolloutStage::Pilot,
            "Broad" => RolloutStage::Broad,
            "Complete" => RolloutStage::Complete,
            "Held" => RolloutStage::Held,
            "Cancelled" => RolloutStage::Cancelled,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolloutInfo {
    pub id: Uuid,
    pub module_id: String,
    #[serde(default)]
    pub from_version: Option<String>,
    pub to_version: String,
    pub stage: RolloutStage,
    #[serde(default)]
    pub hold_reason: Option<String>,
    #[serde(default)]
    pub impact_report_id: Option<Uuid>,
    pub broken_workflows: i32,
    #[serde(default)]
    pub pilot_users: Vec<Uuid>,
    /// Groups (roles) in the broad stage; empty = everyone.
    #[serde(default)]
    pub broad_roles: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRolloutRequest {
    pub module_id: String,
    #[serde(default)]
    pub from_version: Option<String>,
    pub to_version: String,
    #[serde(default)]
    pub impact_report_id: Option<Uuid>,
    #[serde(default)]
    pub broken_workflows: i32,
    #[serde(default)]
    pub immediate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdvanceRolloutRequest {
    #[serde(default)]
    pub force: bool,
}

/// Payload of `modules.version.available`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionAvailable {
    pub module_id: String,
    pub version: String,
    #[serde(default)]
    pub previous_version: Option<String>,
    pub stage: RolloutStage,
    pub hot_swap: HotSwapMode,
    /// Users that receive this version now (empty = everyone).
    #[serde(default)]
    pub users: Vec<Uuid>,
}

/// Gateway route table entry provided by the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteInfo {
    pub prefix: String,
    pub target: String,
    pub module_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::input::{InputField, InputSchema};

    fn field(name: &str, t: FieldType, required: bool) -> InputField {
        InputField { name: name.into(), field_type: t, required, ..InputField::default() }
    }

    fn step(id: &str, fields: Vec<InputField>) -> StepTypeContract {
        StepTypeContract {
            id: id.into(),
            version: 1,
            title: id.into(),
            input: InputSchema { schema_id: id.into(), version: 1, fields },
            outputs: vec!["out".into()],
            headless: true,
            endpoint: format!("/api/x/workflow-steps/{id}"),
        }
    }

    #[test]
    fn ranges() {
        assert!(parse_range(">=1.0.0 <3.0.0").unwrap().matches(&Version::parse("2.5.0").unwrap()));
        assert!(!parse_range(">= 1.0.0 < 3.0.0").unwrap().matches(&Version::parse("3.0.0").unwrap()));
        assert!(parse_range("^1.2").unwrap().matches(&Version::parse("1.9.0").unwrap()));
        assert!(parse_range("*").unwrap().matches(&Version::parse("9.0.0").unwrap()));
    }

    #[test]
    fn diff_detects_removed_renamed_and_required() {
        let old = ContractSurface {
            step_types: vec![step("inv.reorder", vec![field("qty", FieldType::Number, true)]), step("inv.gone", vec![])],
            events: vec![EventContract { event_type: "inv.low".into(), payload_version: 1, fields: vec![] }],
            ..Default::default()
        };
        let mut new = ContractSurface {
            step_types: vec![step("inv.placeOrder", vec![field("amount", FieldType::Number, true), field("note", FieldType::Text, true)])],
            events: vec![EventContract { event_type: "inv.low".into(), payload_version: 2, fields: vec![] }],
            ..Default::default()
        };
        new.migrations.push(ContractMigration {
            kind: ElementKind::StepType,
            from: "inv.reorder".into(),
            to: "inv.placeOrder".into(),
            field_renames: [("qty".to_string(), "amount".to_string())].into(),
            output_renames: BTreeMap::new(),
        });
        let changes = diff_surfaces(&old, &new);
        let renamed = changes.iter().find(|c| c.id == "inv.reorder").unwrap();
        assert_eq!(renamed.kind, ChangeKind::Renamed { to: "inv.placeOrder".into() });
        // new required field 'note' → review
        assert_eq!(renamed.severity, ChangeSeverity::ReviewRequired);
        let gone = changes.iter().find(|c| c.id == "inv.gone").unwrap();
        assert_eq!(gone.severity, ChangeSeverity::Breaking);
        let ev = changes.iter().find(|c| c.id == "inv.low").unwrap();
        assert_eq!(ev.severity, ChangeSeverity::ReviewRequired);
    }

    #[test]
    fn in_place_field_rename_is_migratable() {
        let schema = |f: &str| InputSchema { schema_id: "x.review".into(), version: 1, fields: vec![field(f, FieldType::Text, false)] };
        let old = ContractSurface { input_schemas: vec![schema("note")], ..Default::default() };
        let mut new = ContractSurface { input_schemas: vec![schema("comment")], ..Default::default() };
        new.migrations.push(ContractMigration {
            kind: ElementKind::InputSchema,
            from: "x.review".into(),
            to: "x.review".into(),
            field_renames: [("note".to_string(), "comment".to_string())].into(),
            output_renames: BTreeMap::new(),
        });
        let c = diff_surfaces(&old, &new);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].severity, ChangeSeverity::Migratable);
        assert_eq!(c[0].field_renames["note"], "comment");
    }

    fn manifest(id: &str, version: &str, deps: &[(&str, &str)]) -> ModuleManifest {
        ModuleManifest {
            module_id: id.into(),
            version: version.into(),
            display_name: id.into(),
            description: String::new(),
            icon: String::new(),
            accent_color: None,
            min_host_version: "1.0.0".into(),
            contracts_version: 1,
            prefix: id.into(),
            client: ClientPackage { package: None, view_pack: "views.json".into(), hot_swap: HotSwapMode::Supported },
            backend: None,
            permissions: vec![],
            dependencies: deps.iter().map(|(m, r)| ModuleDependency { module_id: m.to_string(), version_range: r.to_string(), optional: false }).collect(),
            languages: vec!["en".into()],
            default_language: "en".into(),
            contract_surface: ContractSurface::default(),
        }
    }

    #[test]
    fn load_order_and_rejections() {
        let ms = vec![manifest("docs", "1.0.0", &[("users", ">=1.0.0 <3.0.0")]), manifest("users", "1.2.0", &[]), manifest("reports", "1.0.0", &[("missing", "^1")]), manifest("old", "1.0.0", &[("users", "^2")])];
        let (order, rejected) = resolve_load_order(&ms);
        let ids: Vec<_> = order.iter().map(|m| m.module_id.as_str()).collect();
        assert_eq!(ids, vec!["users", "docs"]);
        assert_eq!(rejected.len(), 2);
    }

    #[test]
    fn host_version_check() {
        let mut m = manifest("docs", "1.0.0", &[]);
        m.min_host_version = "2.0.0".into();
        assert_eq!(check_manifest(&m, "1.0.0").unwrap_err().code, "modules.host.tooOld");
    }
}
