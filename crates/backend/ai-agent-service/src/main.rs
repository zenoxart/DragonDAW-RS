//! Red Hive AI agent service: fills workflow steps within guardrails.
//!
//! Guardrails before the model is asked: global and tenant kill switches, the workflow's autonomy and allow
//! list, daily run limit, steps/tokens/time per run, loop detection. After the answer: schema validation,
//! valid options, minimum confidence. Every decision is written to the AI audit (with reasoning, confidence,
//! a hash of the prompt and the tokens used). The agent acts with the RunAs user's delegated token only.

mod providers;

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::Router;
use chrono::{Duration, Utc};
use providers::{Context, Provider};
use redhive_backend_core::db::DbErrorExt;
use redhive_backend_core::hosting::{serve, Core};
use redhive_backend_core::{ApiResult, AuditLog, InternalCall, OutboxWriter, RequestContext, ToHttp, ReqJson};
use redhive_contracts::api::{AiAuditEntry, AiStatus, AiSwitchRequest};
use redhive_contracts::modules::{ModuleManifest, StepTypeContract};
use redhive_contracts::notifications::{event_types, NotificationAudience, Severity};
use redhive_contracts::workflows::api::{AiStepRequest, AiStepResponse};
use redhive_contracts::workflows::expr::{self, Scope};
use redhive_contracts::workflows::*;
use redhive_monads::{Error, OptionExt, Res};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

const SERVICE: &str = "ai-agent";

#[derive(Clone)]
struct AppState {
    core: Core,
    provider: Arc<Provider>,
    outbox: OutboxWriter,
    audit: AuditLog,
}

impl axum::extract::FromRef<AppState> for Core {
    fn from_ref(s: &AppState) -> Core {
        s.core.clone()
    }
}

struct AuditRecord<'a> {
    req: &'a AiStepRequest,
    decision: &'a str,
    policy_code: Option<&'a str>,
    proposal: Option<Value>,
    reasoning: Option<String>,
    confidence: Option<f64>,
    tokens: i64,
    prompt: &'a str,
}

async fn write_audit(s: &AppState, r: AuditRecord<'_>) -> Res<Uuid> {
    let id = Uuid::new_v4();
    let hash: String = Sha256::digest(r.prompt.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query(
        "insert into ai.audit (id, tenant_id, instance_id, workflow_id, step_id, provider, model, model_profile, prompt_hash, proposal, reasoning, confidence, decision, policy_code, tokens)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(id)
    .bind(r.req.instance.tenant_id)
    .bind(r.req.instance.id)
    .bind(r.req.instance.workflow_id)
    .bind(&r.req.step_id)
    .bind(s.provider.name())
    .bind(s.provider.model())
    .bind(&r.req.definition.ai_policy.model_profile)
    .bind(hash)
    .bind(r.proposal)
    .bind(r.reasoning)
    .bind(r.confidence)
    .bind(r.decision)
    .bind(r.policy_code)
    .bind(r.tokens as i32)
    .execute(s.core.db.pool())
    .await
    .db_err("ai.audit")?;
    Ok(id)
}

async fn global_enabled(core: &Core) -> Res<bool> {
    Ok(sqlx::query_scalar::<_, bool>("select enabled from ai.switches where scope = 'global'").fetch_optional(core.db.pool()).await.db_err("ai.switch")?.unwrap_or(true))
}

async fn tenant_enabled(core: &Core, tenant: Uuid) -> bool {
    let v: Res<Value> = core.services.get_internal("configuration-service", &format!("/internal/settings/ai.enabled?tenant={tenant}")).await;
    !matches!(v, Ok(Value::Bool(false)))
}

/// Checks before the model is asked.
async fn pre_checks(s: &AppState, req: &AiStepRequest, step: &WorkflowStep) -> Res<()> {
    let policy = &req.definition.ai_policy;
    if !global_enabled(&s.core).await? {
        return Err(Error::forbidden("ai.killSwitch.global", "ai:disabled"));
    }
    if !tenant_enabled(&s.core, req.instance.tenant_id).await {
        return Err(Error::forbidden("ai.killSwitch.tenant", "ai:disabled"));
    }
    if !policy.allows(step) {
        return Err(Error::forbidden("ai.policy.step", "ai:stepNotAllowed").with_metadata("step", step.id.clone()));
    }
    let runs_today: i64 = sqlx::query_scalar("select count(distinct instance_id) from ai.audit where workflow_id = $1 and at >= date_trunc('day', now()) and instance_id <> $2")
        .bind(req.instance.workflow_id)
        .bind(req.instance.id)
        .fetch_one(s.core.db.pool())
        .await
        .db_err("ai.usage")?;
    let entries: i64 = sqlx::query_scalar("select count(*) from ai.audit where instance_id = $1").bind(req.instance.id).fetch_one(s.core.db.pool()).await.db_err("ai.usage")?;
    let inst = &req.instance;
    match () {
        _ if runs_today >= policy.max_runs_per_day => Err(Error::forbidden("ai.limit.runsPerDay", "ai:limitRunsPerDay").with_metadata("limit", policy.max_runs_per_day)),
        _ if entries >= policy.max_steps_per_run => Err(Error::forbidden("ai.limit.stepsPerRun", "ai:limitSteps").with_metadata("limit", policy.max_steps_per_run)),
        _ if inst.ai_tokens >= policy.max_tokens_per_run => Err(Error::forbidden("ai.limit.tokens", "ai:limitTokens").with_metadata("limit", policy.max_tokens_per_run)),
        _ if inst.ai_entries(&step.id) >= 3 => Err(Error::forbidden("ai.loop", "ai:loopDetected")),
        _ if Utc::now() - inst.created_at > Duration::minutes(policy.time_limit_minutes) => Err(Error::forbidden("ai.limit.time", "ai:limitTime")),
        _ => Ok(()),
    }
}

/// Variables the AI may see: the data scope (or all), without sensitive variables and fields.
fn data_scope(req: &AiStepRequest, step: &WorkflowStep) -> Map<String, Value> {
    let policy = &req.definition.ai_policy;
    let sensitive: Vec<&str> = req
        .definition
        .variables
        .iter()
        .filter(|v| v.sensitive)
        .map(|v| v.name.as_str())
        .chain(step.input.iter().flat_map(|i| i.fields.iter().filter(|f| f.sensitive).map(|f| f.name.as_str())))
        .collect();
    req.instance
        .vars
        .iter()
        .filter(|(k, _)| !k.starts_with("__") && !sensitive.contains(&k.as_str()))
        .filter(|(k, _)| policy.data_scope.is_empty() || policy.data_scope.contains(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn engine_scope(inst: &WorkflowInstance) -> Scope {
    let payload = inst.trigger.as_ref().map(|t| t.payload.clone()).unwrap_or(Value::Null);
    Scope::new()
        .with("vars", Value::Object(inst.vars.iter().filter(|(k, _)| !k.starts_with("__")).map(|(k, v)| (k.clone(), v.clone())).collect()))
        .with("steps", Value::Object(inst.steps.clone()))
        .with("last", inst.last.clone())
        .with("payload", payload.clone())
        .with("trigger", payload)
}

async fn tools_for(s: &AppState, step: &WorkflowStep) -> Res<Vec<StepTypeContract>> {
    if step.tools.is_empty() {
        return Ok(vec![]);
    }
    let manifests: Vec<ModuleManifest> = s.core.services.get_internal("module-registry", "/internal/manifests").await?;
    Ok(manifests.into_iter().flat_map(|m| m.contract_surface.step_types).filter(|t| step.tools.contains(&t.id) && t.headless).collect())
}

/// Workflow service → agent: propose the result of an interactive step.
async fn propose(State(s): State<AppState>, _call: InternalCall, ReqJson(req): ReqJson<AiStepRequest>) -> ApiResult<AiStepResponse> {
    async {
        let step = req.definition.step(&req.step_id).cloned().ok_or_not_found("ai.step", "workflow:step.missing")?;
        if let Err(e) = pre_checks(&s, &req, &step).await {
            write_audit(&s, AuditRecord { req: &req, decision: "blocked", policy_code: Some(&e.code), proposal: None, reasoning: None, confidence: None, tokens: 0, prompt: "" }).await?;
            return Err(e);
        }
        let prefill: Map<String, Value> = step.parameters.iter().filter_map(|(k, src)| expr::eval_str(src, &engine_scope(&req.instance)).ok().filter(|v| !v.is_null()).map(|v| (k.clone(), v))).collect();
        let tools = tools_for(&s, &step).await.unwrap_or_default();
        let ctx = Context { step: &step, workflow_name: &req.definition.name, data: data_scope(&req, &step), prefill, tools, access_token: &req.access_token, services: &s.core.services };
        let answer = match s.provider.answer(&ctx).await {
            Ok(a) => a,
            Err(e) => {
                write_audit(&s, AuditRecord { req: &req, decision: "providerError", policy_code: Some(&e.code), proposal: None, reasoning: e.message.clone(), confidence: None, tokens: 0, prompt: "" }).await?;
                return Err(e);
            }
        };
        let proposal = AiProposal { values: answer.values.clone(), decision: answer.decision.clone(), confidence: answer.confidence, reasoning: answer.reasoning.clone(), provider: s.provider.name().into(), model: s.provider.model(), created_at: Utc::now() };
        let proposal_json = serde_json::to_value(&proposal).unwrap_or_default();

        // After the answer: schema, options, confidence.
        let mut post_check: Res<()> = Ok(());
        if let Some(schema) = &step.input {
            if let Err(e) = schema.validate(&answer.values) {
                post_check = Err(Error::validation("ai.proposal.invalid", "ai:proposalInvalid").with_message(e.message.clone().unwrap_or_default()).with_metadata("fieldErrors", serde_json::to_value(&e.field_errors).unwrap_or_default()));
            }
        }
        if let (Ok(()), Some(d)) = (&post_check, &answer.decision) {
            if step.kind == StepKind::Choice && !step.options.iter().any(|o| &o.value == d) {
                post_check = Err(Error::validation("ai.proposal.option", "ai:proposalInvalid").with_metadata("decision", d.clone()));
            }
        }
        if post_check.is_ok() && answer.confidence < req.definition.ai_policy.required_confidence {
            post_check = Err(Error::forbidden("ai.confidence.low", "ai:lowConfidence").with_metadata("required", req.definition.ai_policy.required_confidence));
        }
        let decision = match &post_check {
            Ok(()) => "proposed",
            Err(e) if e.code == "ai.confidence.low" => "lowConfidence",
            Err(_) => "invalid",
        };
        let audit_id = write_audit(
            &s,
            AuditRecord {
                req: &req,
                decision,
                policy_code: post_check.as_ref().err().map(|e| e.code.as_str()),
                proposal: Some(json!({ "proposal": proposal_json, "tools": answer.tool_calls })),
                reasoning: Some(answer.reasoning.clone()),
                confidence: Some(answer.confidence),
                tokens: answer.tokens,
                prompt: &answer.prompt,
            },
        )
        .await?;
        tracing::info!(instance = %req.instance.id, step = %step.id, decision, confidence = answer.confidence, "AI proposal");
        match post_check {
            Ok(()) => Ok(AiStepResponse { proposal, tool_calls: answer.tool_calls, tokens: answer.tokens, audit_id }),
            // The engine hands over to a person and shows the proposal as a hint.
            Err(e) => Err(e.with_metadata("proposal", proposal_json).with_metadata("confidence", answer.confidence).with_metadata("reasoning", answer.reasoning)),
        }
    }
    .await
    .http()
}

async fn status(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<AiStatus> {
    async {
        let runs_today: i64 = sqlx::query_scalar("select count(distinct instance_id) from ai.audit where tenant_id = $1 and at >= date_trunc('day', now())")
            .bind(ctx.user.tenant_id)
            .fetch_one(s.core.db.pool())
            .await
            .db_err("ai.status")?;
        Ok(AiStatus { enabled: global_enabled(&s.core).await? && tenant_enabled(&s.core, ctx.user.tenant_id).await, provider: s.provider.name().into(), model: s.provider.model(), runs_today })
    }
    .await
    .http()
}

/// The kill switch (global or tenant).
async fn switch(State(s): State<AppState>, ctx: RequestContext, ReqJson(req): ReqJson<AiSwitchRequest>) -> ApiResult<AiStatus> {
    async {
        ctx.user.require("ai.admin")?;
        match req.scope.as_str() {
            "global" => {
                let mut tx = s.core.db.begin().await?;
                sqlx::query("insert into ai.switches (scope, enabled, changed_by) values ('global', $1, $2) on conflict (scope) do update set enabled = excluded.enabled, changed_at = now(), changed_by = excluded.changed_by")
                    .bind(req.enabled)
                    .bind(ctx.user.audit_name())
                    .execute(&mut *tx)
                    .await
                    .db_err("ai.switch")?;
                let ev = s
                    .outbox
                    .create(event_types::AI_SWITCH_CHANGED, &ctx.user, NotificationAudience::WithPermission("ai.run".into()), json!({ "enabled": req.enabled, "scope": "global" }))
                    .titled(if req.enabled { "ai:notification.enabled" } else { "ai:notification.disabled" }, if req.enabled { Severity::Info } else { Severity::Warning });
                s.outbox.add(&mut tx, &ev).await?;
                s.audit.write(&mut tx, &ctx.user, if req.enabled { "ai.enabled" } else { "ai.disabled" }, "ai", "global", json!({})).await?;
                tx.commit().await.db_err("ai.commit")?;
            }
            "tenant" => {
                let _: Value = s.core.services.put("configuration-service", "/api/config/tenant/ai.enabled", &json!({ "value": req.enabled }), &redhive_backend_core::Auth::Bearer(ctx.token.clone())).await?;
            }
            _ => return Err(Error::validation("ai.scope.invalid", "ai:scope.invalid").for_field("scope")),
        }
        status(State(s.clone()), ctx).await.map(|j| j.0).map_err(|e| e.0)
    }
    .await
    .http()
}

#[derive(Deserialize, Default)]
struct AuditQuery {
    #[serde(default)]
    instance: Option<Uuid>,
}

async fn audit(State(s): State<AppState>, ctx: RequestContext, Query(q): Query<AuditQuery>) -> ApiResult<Vec<AiAuditEntry>> {
    async {
        if !ctx.user.has("ai.approve") && !ctx.user.has("ai.admin") && !ctx.user.has("workflows.edit") {
            return Err(Error::forbidden("auth.permission.missing", "errors:forbidden").with_metadata("permission", "ai.approve"));
        }
        let rows = sqlx::query(
            "select id, instance_id, workflow_id, step_id, at, provider, model, decision, reasoning, confidence, proposal, policy_code, tokens
             from ai.audit where tenant_id = $1 and ($2::uuid is null or instance_id = $2) order by at desc limit 200",
        )
        .bind(ctx.user.tenant_id)
        .bind(q.instance)
        .fetch_all(s.core.db.pool())
        .await
        .db_err("ai.audit")?;
        Ok(rows
            .into_iter()
            .map(|r| AiAuditEntry {
                id: r.get("id"),
                instance_id: r.get("instance_id"),
                workflow_id: r.get("workflow_id"),
                step_id: r.get("step_id"),
                at: r.get("at"),
                provider: r.get("provider"),
                model: r.get("model"),
                decision: r.get("decision"),
                reasoning: r.get("reasoning"),
                confidence: r.get("confidence"),
                proposal: r.get("proposal"),
                policy_code: r.get("policy_code"),
                tokens: r.get("tokens"),
            })
            .collect())
    }
    .await
    .http()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let core = Core::init(SERVICE).await?;
    let o = &core.options;
    let provider = match o.var("AI__PROVIDER").as_deref() {
        Some("openai") => Provider::OpenAi {
            endpoint: o.var("AI__ENDPOINT").unwrap_or_else(|| "https://api.openai.com/v1".into()),
            api_key: o.var("AI__API_KEY").ok_or("REDHIVE__AI__API_KEY is required for the openai provider")?,
            model: o.var("AI__MODEL").unwrap_or_else(|| "gpt-4o-mini".into()),
            http: reqwest::Client::builder().timeout(std::time::Duration::from_secs(60)).build()?,
        },
        _ => Provider::Rules { review_limit: o.var("AI__REVIEW_LIMIT").and_then(|v| v.parse().ok()).unwrap_or(20_000.0) },
    };
    tracing::info!(provider = provider.name(), model = %provider.model(), "AI provider");
    let state = AppState { core: core.clone(), provider: Arc::new(provider), outbox: OutboxWriter::new(SERVICE), audit: AuditLog::new(SERVICE) };
    let router = Router::new()
        .route("/api/ai/status", get(status))
        .route("/api/ai/switch", post(switch))
        .route("/api/ai/audit", get(audit))
        .route("/internal/propose", post(propose));
    serve(core, router, state).await
}
