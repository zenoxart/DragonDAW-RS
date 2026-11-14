//! The module catalogue (latest manifests from the registry), cached for 30 seconds.
//! Used to resolve step types and commands to endpoints and to extract dependencies.

use redhive_backend_core::hosting::Core;
use redhive_contracts::modules::{CommandContract, ModuleManifest, StepTypeContract};
use redhive_monads::{Error, Res};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct Catalog {
    inner: Arc<RwLock<Option<(Instant, Vec<ModuleManifest>)>>>,
}

impl Catalog {
    pub async fn manifests(&self, core: &Core) -> Res<Vec<ModuleManifest>> {
        if let Some((at, m)) = self.inner.read().await.as_ref() {
            if at.elapsed() < Duration::from_secs(30) {
                return Ok(m.clone());
            }
        }
        let fresh: Vec<ModuleManifest> = core.services.get_internal("module-registry", "/internal/manifests").await?;
        *self.inner.write().await = Some((Instant::now(), fresh.clone()));
        Ok(fresh)
    }

    /// Forces a reload (after a publish).
    pub async fn invalidate(&self) {
        *self.inner.write().await = None;
    }

    pub async fn step_type(&self, core: &Core, id: &str) -> Res<StepTypeContract> {
        self.manifests(core)
            .await?
            .into_iter()
            .flat_map(|m| m.contract_surface.step_types)
            .find(|s| s.id == id)
            .ok_or_else(|| Error::not_found("workflow.stepType.unknown", "workflow:stepType.unknown").with_metadata("stepType", id.to_string()))
    }

    pub async fn command(&self, core: &Core, id: &str) -> Res<CommandContract> {
        self.manifests(core)
            .await?
            .into_iter()
            .flat_map(|m| m.contract_surface.commands)
            .find(|c| c.id == id)
            .ok_or_else(|| Error::not_found("workflow.command.unknown", "workflow:command.unknown").with_metadata("command", id.to_string()))
    }
}

/// What the designer can use: step types, commands, events, input schemas.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignerCatalog {
    pub modules: Vec<DesignerModule>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignerModule {
    pub module_id: String,
    pub version: String,
    pub display_name: String,
    pub step_types: Vec<StepTypeContract>,
    pub commands: Vec<CommandContract>,
    pub events: Vec<redhive_contracts::modules::EventContract>,
    pub input_schemas: Vec<redhive_contracts::workflows::input::InputSchema>,
}

pub fn designer(manifests: Vec<ModuleManifest>) -> DesignerCatalog {
    DesignerCatalog {
        modules: manifests
            .into_iter()
            .map(|m| DesignerModule {
                module_id: m.module_id,
                version: m.version,
                display_name: m.display_name,
                step_types: m.contract_surface.step_types,
                commands: m.contract_surface.commands.into_iter().filter(|c| c.workflow_capable).collect(),
                events: m.contract_surface.events,
                input_schemas: m.contract_surface.input_schemas,
            })
            .collect(),
    }
}
