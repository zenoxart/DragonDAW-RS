//! Impact analysis of module updates: classify, migrate, flag and report.

use crate::engine::Engine;
use crate::store;
use chrono::Utc;
use redhive_contracts::notifications::{event_types, NotificationAudience, NotificationEnvelope, Severity};
use redhive_contracts::primitives::ids;
use redhive_contracts::workflows::api::{AnalyzeImpactRequest, AnalyzeImpactResponse};
use redhive_contracts::workflows::impact::{analyze, suspend_ai_triggers, ImpactClass, ImpactReport};
use redhive_contracts::workflows::{DependencyKind, WorkflowStatus};
use redhive_monads::Res;
use serde_json::json;
use uuid::Uuid;

/// Analyses every workflow against the changes; with `apply` it saves migrated versions, flags workflows
/// that need review (AI triggers suspended, running instances paused) or are broken, and stores the report.
pub async fn run(engine: &Engine, req: AnalyzeImpactRequest) -> Res<AnalyzeImpactResponse> {
    let mut report = ImpactReport { id: Uuid::new_v4(), module_id: req.module_id.clone(), from_version: req.from_version.clone(), to_version: req.to_version.clone(), analyzed_at: Utc::now(), changes: req.changes.clone(), workflows: Vec::new() };
    let tenant = ids::DEMO_TENANT;
    let changed_ids: Vec<&str> = req.changes.iter().map(|c| c.id.as_str()).collect();
    for def in store::latest_all(&engine.core, tenant).await? {
        if def.status == WorkflowStatus::Retired {
            continue;
        }
        // Quick filter: only workflows that depend on the module or on a changed element.
        let relevant = def.dependencies.iter().any(|d| (d.kind == DependencyKind::Module && d.id == req.module_id) || changed_ids.contains(&d.id.as_str()))
            || def.steps.iter().any(|s| s.step_type_id.as_deref().is_some_and(|x| changed_ids.contains(&x)) || s.command_id.as_deref().is_some_and(|x| changed_ids.contains(&x)) || s.input.as_ref().is_some_and(|i| changed_ids.contains(&i.schema_id.as_str())))
            || def.triggers.iter().any(|t| t.event_type.as_deref().is_some_and(|x| changed_ids.contains(&x)));
        if !relevant {
            continue;
        }
        let analysis = analyze(&def, &req.changes);
        let mut impact = analysis.impact;
        if req.apply && impact.class != ImpactClass::Compatible {
            let mut tx = engine.core.db.begin().await?;
            match impact.class {
                ImpactClass::AutoMigrated | ImpactClass::ReviewRequired => {
                    let mut migrated = match analysis.migrated {
                        Some(m) => m,
                        None => {
                            let mut m = def.clone();
                            m.updated_at = Some(Utc::now());
                            m
                        }
                    };
                    migrated.version = store::next_version(&mut tx, def.id).await?;
                    if impact.class == ImpactClass::ReviewRequired {
                        migrated.status = WorkflowStatus::NeedsReview;
                        migrated.review_reasons = impact.reasons.clone();
                        let n = suspend_ai_triggers(&mut migrated);
                        if n > 0 {
                            impact.actions.push(format!("{n} AI trigger(s) suspended"));
                        }
                    }
                    let catalog = engine.catalog.manifests(&engine.core).await.unwrap_or_default();
                    migrated.dependencies = redhive_contracts::workflows::analysis::dependencies(&migrated, &catalog);
                    store::insert(&mut tx, tenant, &migrated).await?;
                    impact.to_version = Some(migrated.version);
                    impact.actions.push(format!("saved as version {}", migrated.version));
                }
                ImpactClass::Broken => {
                    let mut broken = def.clone();
                    broken.version = store::next_version(&mut tx, def.id).await?;
                    broken.status = WorkflowStatus::Broken;
                    broken.review_reasons = impact.reasons.clone();
                    let n = suspend_ai_triggers(&mut broken);
                    for t in broken.triggers.iter_mut() {
                        t.suspended = true;
                    }
                    store::insert(&mut tx, tenant, &broken).await?;
                    impact.to_version = Some(broken.version);
                    impact.actions.push(format!("marked as broken, all triggers suspended ({n} AI)"));
                }
                ImpactClass::Compatible => {}
            }
            tx.commit().await.map_err(|e| redhive_backend_core::db::map_db_error("workflow.impact.commit", e))?;
            if impact.class >= ImpactClass::ReviewRequired {
                let paused = engine.pause_instances(def.id, &format!("module {} {}", req.module_id, req.to_version)).await?;
                if paused > 0 {
                    impact.actions.push(format!("{paused} running instance(s) paused"));
                }
            }
        }
        report.workflows.push(impact);
    }
    let resp = AnalyzeImpactResponse {
        report_id: if req.apply { report.id } else { Uuid::nil() },
        compatible: report.count(ImpactClass::Compatible),
        auto_migrated: report.count(ImpactClass::AutoMigrated),
        review_required: report.count(ImpactClass::ReviewRequired),
        broken: report.count(ImpactClass::Broken),
    };
    if req.apply {
        store::save_report(&engine.core, &report).await?;
        engine.catalog.invalidate().await;
        let mut env = NotificationEnvelope::new(event_types::WORKFLOW_IMPACT_ANALYZED, crate::engine::SOURCE, tenant, NotificationAudience::WithPermission("workflows.edit".into()), serde_json::to_value(&resp).unwrap_or_default())
            .titled(if resp.broken > 0 || resp.review_required > 0 { "workflow:notification.impactAttention" } else { "workflow:notification.impact" }, if resp.broken > 0 { Severity::Warning } else { Severity::Info })
            .navigate("workflows.impact", Some(report.id.to_string()));
        env.text_args.insert("module".into(), json!(req.module_id));
        env.text_args.insert("version".into(), json!(req.to_version));
        env.text_args.insert("migrated".into(), json!(resp.auto_migrated));
        env.text_args.insert("review".into(), json!(resp.review_required));
        env.text_args.insert("broken".into(), json!(resp.broken));
        if let Err(e) = engine.outbox.add_now(&engine.core, &env).await {
            tracing::warn!(error = %e, "impact event could not be written");
        }
        tracing::info!(module = %req.module_id, version = %req.to_version, migrated = resp.auto_migrated, review = resp.review_required, broken = resp.broken, "impact analysis applied");
    }
    Ok(resp)
}
