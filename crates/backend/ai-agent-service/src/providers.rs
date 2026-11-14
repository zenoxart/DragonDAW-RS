//! AI providers: an offline **rules** provider (deterministic, no network) and any **OpenAI-compatible**
//! chat-completions endpoint with tool calling and JSON output.
//!
//! Both act only through *tools* – the step types listed in the step's `tools` – executed through the gateway
//! with the delegated token of the RunAs user, i.e. with exactly that user's permissions.

use redhive_backend_core::{Auth, ServiceClient};
use redhive_contracts::modules::StepTypeContract;
use redhive_contracts::workflows::api::AiToolCall;
use redhive_contracts::workflows::input::{FieldType, InputSchema};
use redhive_contracts::workflows::{StepKind, WorkflowStep};
use redhive_monads::{Error, Res};
use serde_json::{json, Map, Value};

/// Everything a provider gets.
pub struct Context<'a> {
    pub step: &'a WorkflowStep,
    pub workflow_name: &'a str,
    /// Variables the AI may see (data scope applied, sensitive ones removed).
    pub data: Map<String, Value>,
    /// Values pre-filled from the step parameters.
    pub prefill: Map<String, Value>,
    pub tools: Vec<StepTypeContract>,
    pub access_token: &'a str,
    pub services: &'a ServiceClient,
}

pub struct Answer {
    pub values: Map<String, Value>,
    pub decision: Option<String>,
    pub confidence: f64,
    pub reasoning: String,
    pub tokens: i64,
    pub tool_calls: Vec<AiToolCall>,
    pub prompt: String,
}

pub enum Provider {
    Rules { review_limit: f64 },
    OpenAi { endpoint: String, api_key: String, model: String, http: reqwest::Client },
}

impl Provider {
    pub fn name(&self) -> &'static str {
        match self {
            Provider::Rules { .. } => "rules",
            Provider::OpenAi { .. } => "openai",
        }
    }

    pub fn model(&self) -> String {
        match self {
            Provider::Rules { .. } => "rules-v1".into(),
            Provider::OpenAi { model, .. } => model.clone(),
        }
    }

    pub async fn answer(&self, ctx: &Context<'_>) -> Res<Answer> {
        match self {
            Provider::Rules { review_limit } => rules(ctx, *review_limit).await,
            Provider::OpenAi { endpoint, api_key, model, http } => openai(ctx, endpoint, api_key, model, http).await,
        }
    }
}

/// Runs a tool (a headless step type) as the RunAs user.
pub async fn call_tool(ctx: &Context<'_>, tool: &StepTypeContract, inputs: Map<String, Value>) -> AiToolCall {
    let validated = tool.input.validate(&inputs);
    let result: Res<Value> = match validated {
        Ok(v) => {
            let url = ctx.services.url("api-gateway", &tool.endpoint);
            ctx.services.send(reqwest::Method::POST, &url, Some(&Value::Object(v)), &Auth::Bearer(ctx.access_token.to_string()), None).await
        }
        Err(e) => Err(e),
    };
    match result {
        Ok(out) => AiToolCall { tool: tool.id.clone(), inputs, outputs: out, error: None },
        Err(e) => AiToolCall { tool: tool.id.clone(), inputs, outputs: Value::Null, error: Some(e.code) },
    }
}

fn describe(schema: &Option<InputSchema>) -> Value {
    match schema {
        None => json!([]),
        Some(s) => Value::Array(
            s.fields
                .iter()
                .filter(|f| !f.sensitive)
                .map(|f| json!({ "name": f.name, "type": f.field_type, "required": f.required, "options": f.options.iter().map(|o| &o.value).collect::<Vec<_>>(), "min": f.min, "max": f.max }))
                .collect(),
        ),
    }
}

fn prompt_text(ctx: &Context<'_>) -> String {
    format!(
        "Workflow: {}\nStep: {} ({:?})\nInstruction: {}\nFields: {}\nKnown data: {}\nPrefilled: {}",
        ctx.workflow_name,
        ctx.step.title,
        ctx.step.kind,
        ctx.step.prompt.clone().unwrap_or_else(|| ctx.step.description.clone()),
        describe(&ctx.step.input),
        Value::Object(ctx.data.clone()),
        Value::Object(ctx.prefill.clone())
    )
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
}

/// Deterministic rules: fetch data with the tools, fill the form, and recommend a decision.
async fn rules(ctx: &Context<'_>, review_limit: f64) -> Res<Answer> {
    let prompt = prompt_text(ctx);
    let mut values = ctx.prefill.clone();
    let mut calls = Vec::new();
    for tool in &ctx.tools {
        // Inputs: from prefill / data by name.
        let mut inputs = Map::new();
        for f in &tool.input.fields {
            if let Some(v) = values.get(&f.name).or_else(|| ctx.data.get(&f.name)) {
                inputs.insert(f.name.clone(), v.clone());
            }
        }
        let call = call_tool(ctx, tool, inputs).await;
        if let Value::Object(out) = &call.outputs {
            for (k, v) in out {
                if ctx.step.input.as_ref().is_some_and(|s| s.field(k).is_some()) && values.get(k).is_none_or(|x| x.is_null() || x == "") {
                    values.insert(k.clone(), v.clone());
                }
            }
        }
        calls.push(call);
    }
    let failed_tools = calls.iter().filter(|c| c.error.is_some()).count();
    let mut confidence: f64 = if failed_tools > 0 { 0.4 } else { 0.85 };
    let mut reasons: Vec<String> = Vec::new();
    let mut decision = None;

    match ctx.step.kind {
        StepKind::AiTask => {
            let summary = ctx.data.iter().filter(|(k, _)| !k.starts_with("__")).map(|(k, v)| format!("{k}: {}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect::<Vec<_>>().join("; ");
            values.insert("result".into(), json!(format!("Summary – {summary}")));
            reasons.push("Summarised the workflow data.".into());
            confidence = 0.8;
        }
        StepKind::Choice => {
            decision = ctx.step.options.first().map(|o| o.value.clone());
            reasons.push("No rule for this choice – picked the first option.".into());
            confidence = 0.5;
        }
        _ => {
            let schema = ctx.step.input.clone().unwrap_or_default();
            // Recommendation rule (invoice-style reviews).
            if let Some(field) = schema.fields.iter().find(|f| f.name == "recommendation" && f.field_type == FieldType::Choice) {
                let amount = num(values.get("amount"));
                let supplier = values.get("supplier").and_then(Value::as_str).unwrap_or("").trim().to_string();
                let rec = match amount {
                    Some(a) if a > review_limit => {
                        reasons.push(format!("The amount {a:.2} is above {review_limit:.2}, the limit for automatic pre-checks – a person should decide."));
                        confidence = confidence.min(0.55);
                        "clarify"
                    }
                    None => {
                        reasons.push("The amount is unknown.".into());
                        confidence = confidence.min(0.4);
                        "clarify"
                    }
                    Some(_) if supplier.is_empty() => {
                        reasons.push("The supplier is missing.".into());
                        confidence = confidence.min(0.6);
                        "clarify"
                    }
                    Some(a) => {
                        reasons.push(format!("Supplier '{supplier}' and amount {a:.2} are plausible and within the limit of {review_limit:.2}."));
                        "approve"
                    }
                };
                if field.options.iter().any(|o| o.value == rec) {
                    values.insert("recommendation".into(), json!(rec));
                    decision = Some(rec.to_string());
                }
                if schema.field("note").is_some() && values.get("note").is_none() {
                    values.insert("note".into(), json!(format!("AI pre-check: {}", reasons.join(" "))));
                }
                if schema.field("comment").is_some() && values.get("comment").is_none() {
                    values.insert("comment".into(), json!(format!("AI pre-check: {}", reasons.join(" "))));
                }
            }
            // Fill the remaining required fields where a safe default exists.
            for f in schema.fields.iter().filter(|f| f.required) {
                if values.get(&f.name).is_some_and(|v| !v.is_null() && v != "") {
                    continue;
                }
                match (&f.default, f.field_type) {
                    (Some(d), _) => {
                        values.insert(f.name.clone(), d.clone());
                    }
                    (None, FieldType::Boolean) => {
                        values.insert(f.name.clone(), json!(false));
                        confidence -= 0.1;
                    }
                    (None, FieldType::Choice) if !f.options.is_empty() => {
                        values.insert(f.name.clone(), json!(f.options[0].value));
                        reasons.push(format!("'{}' set to the first option.", f.name));
                        confidence -= 0.2;
                    }
                    _ => {
                        reasons.push(format!("No value for the required field '{}'.", f.name));
                        confidence = confidence.min(0.3);
                    }
                }
            }
        }
    }
    if failed_tools > 0 {
        reasons.push(format!("{failed_tools} tool call(s) failed."));
    }
    if reasons.is_empty() {
        reasons.push("All required data was available.".into());
    }
    let tokens = (prompt.len() / 4) as i64;
    Ok(Answer { values, decision, confidence: confidence.clamp(0.0, 1.0), reasoning: reasons.join(" "), tokens, tool_calls: calls, prompt })
}

fn tool_name(id: &str) -> String {
    id.replace('.', "__")
}

/// OpenAI-compatible chat completions with function calling (max. 4 tool rounds) and JSON output.
async fn openai(ctx: &Context<'_>, endpoint: &str, api_key: &str, model: &str, http: &reqwest::Client) -> Res<Answer> {
    let prompt = prompt_text(ctx);
    let system = "You are the Red Hive workflow agent. You fill workflow steps for a business user. \
        Use the tools to look up data; never invent data. Answer ONLY with a JSON object \
        {\"values\": {<field>: <value>}, \"decision\": <option or null>, \"confidence\": <0..1>, \"reasoning\": <short text>}. \
        Lower the confidence when data is missing or a person should decide.";
    let mut messages = vec![json!({ "role": "system", "content": system }), json!({ "role": "user", "content": prompt })];
    let tools: Vec<Value> = ctx
        .tools
        .iter()
        .map(|t| {
            let props: Map<String, Value> = t.input.fields.iter().map(|f| (f.name.clone(), json!({ "type": if matches!(f.field_type, FieldType::Number | FieldType::Money) { "number" } else if f.field_type == FieldType::Boolean { "boolean" } else { "string" } }))).collect();
            let required: Vec<&String> = t.input.fields.iter().filter(|f| f.required).map(|f| &f.name).collect();
            json!({ "type": "function", "function": { "name": tool_name(&t.id), "description": t.title, "parameters": { "type": "object", "properties": props, "required": required } } })
        })
        .collect();
    let mut calls = Vec::new();
    let mut tokens = 0i64;
    let unavailable = |m: String| Error::unavailable("ai.provider.unavailable", "ai:provider.unavailable").with_message(m);
    for _round in 0..5 {
        let mut body = json!({ "model": model, "messages": messages, "temperature": 0.1, "response_format": { "type": "json_object" } });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.clone());
        }
        let resp = http.post(format!("{}/chat/completions", endpoint.trim_end_matches('/'))).bearer_auth(api_key).json(&body).send().await.map_err(|e| unavailable(e.to_string()))?;
        let status = resp.status();
        let v: Value = resp.json().await.map_err(|e| unavailable(e.to_string()))?;
        if !status.is_success() {
            return Err(unavailable(format!("{status}: {}", v["error"]["message"].as_str().unwrap_or(""))));
        }
        tokens += v["usage"]["total_tokens"].as_i64().unwrap_or(0);
        let msg = v["choices"][0]["message"].clone();
        if let Some(tc) = msg["tool_calls"].as_array().filter(|a| !a.is_empty()) {
            messages.push(msg.clone());
            for c in tc {
                let name = c["function"]["name"].as_str().unwrap_or("");
                let args: Map<String, Value> = serde_json::from_str(c["function"]["arguments"].as_str().unwrap_or("{}")).unwrap_or_default();
                let content = match ctx.tools.iter().find(|t| tool_name(&t.id) == name) {
                    Some(t) => {
                        let call = call_tool(ctx, t, args).await;
                        let content = json!({ "outputs": call.outputs, "error": call.error }).to_string();
                        calls.push(call);
                        content
                    }
                    None => json!({ "error": "unknown tool" }).to_string(),
                };
                messages.push(json!({ "role": "tool", "tool_call_id": c["id"], "content": content }));
            }
            continue;
        }
        let text = msg["content"].as_str().unwrap_or("{}");
        let parsed: Value = serde_json::from_str(text).map_err(|e| Error::validation("ai.answer.invalid", "ai:answer.invalid").with_message(e.to_string()))?;
        let mut values = ctx.prefill.clone();
        if let Some(o) = parsed["values"].as_object() {
            values.extend(o.clone());
        }
        return Ok(Answer {
            values,
            decision: parsed["decision"].as_str().map(str::to_string),
            confidence: parsed["confidence"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0),
            reasoning: parsed["reasoning"].as_str().unwrap_or("").to_string(),
            tokens,
            tool_calls: calls,
            prompt,
        });
    }
    Err(Error::validation("ai.answer.toolLoop", "ai:answer.toolLoop"))
}
