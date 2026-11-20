// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use super::context::AssembledContext;
use super::routing::TaskClass;

#[derive(Debug, Clone, Serialize)]
pub struct ZyraAgentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub task_class: String,
}

pub fn catalog() -> Vec<ZyraAgentInfo> {
    vec![
        agent(
            "auto",
            "Auto",
            "Zyra picks the best specialist for your request.",
            TaskClass::Infrastructure,
        ),
        agent(
            "architect",
            "Zyra Architect",
            "Designs infrastructure and environments.",
            TaskClass::Infrastructure,
        ),
        agent(
            "devops",
            "Zyra DevOps",
            "CI/CD, runbooks, and operational automation.",
            TaskClass::CodeGeneration,
        ),
        agent(
            "kubernetes",
            "Zyra Kubernetes",
            "Cluster and workload operations.",
            TaskClass::Infrastructure,
        ),
        agent(
            "security",
            "Zyra Security",
            "Threat detection and security posture.",
            TaskClass::SecurityAnalysis,
        ),
        agent(
            "cost",
            "Zyra Cost Optimizer",
            "Cloud cost analysis and FinOps.",
            TaskClass::Research,
        ),
        agent(
            "observability",
            "Zyra Observability",
            "Logs, metrics, traces, and root cause.",
            TaskClass::LongContext,
        ),
        agent(
            "sre",
            "Zyra SRE",
            "Incident response and VM health.",
            TaskClass::LongContext,
        ),
        agent(
            "ai_engineer",
            "Zyra AI Engineer",
            "LLM deployment and inference stacks.",
            TaskClass::CodeGeneration,
        ),
        agent(
            "database",
            "Zyra Database Expert",
            "Database tuning and optimization.",
            TaskClass::LongContext,
        ),
    ]
}

fn agent(id: &str, name: &str, description: &str, task: TaskClass) -> ZyraAgentInfo {
    ZyraAgentInfo {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        task_class: task.as_db_key().into(),
    }
}

pub fn resolve_agent_id(raw: Option<&str>) -> String {
    let id = raw.unwrap_or("auto").trim().to_ascii_lowercase();
    if id == "auto" {
        return "auto".into();
    }
    if catalog().iter().any(|a| a.id == id) {
        return id;
    }
    "auto".into()
}

pub fn pick_agent(message: &str, page_hint: Option<&str>) -> String {
    let ml = message.to_ascii_lowercase();
    let page = page_hint.unwrap_or("").to_ascii_lowercase();
    if ml.contains("kubernetes") || ml.contains("k8s") || ml.contains("pod") || page.contains("k8s")
    {
        return "kubernetes".into();
    }
    if ml.contains("security")
        || ml.contains("firewall")
        || ml.contains("threat")
        || page.contains("security")
    {
        return "security".into();
    }
    if ml.contains("cost") || ml.contains("finops") || ml.contains("budget") {
        return "cost".into();
    }
    if ml.contains("terraform")
        || ml.contains("ansible")
        || ml.contains("deploy")
        || ml.contains("pipeline")
    {
        return "devops".into();
    }
    if ml.contains("latency")
        || ml.contains("incident")
        || ml.contains("doctor")
        || ml.contains("health")
    {
        return "sre".into();
    }
    if ml.contains("database") || ml.contains("postgres") || ml.contains("mysql") {
        return "database".into();
    }
    if ml.contains("vllm")
        || ml.contains("ollama")
        || ml.contains("inference")
        || ml.contains("gpu")
    {
        return "ai_engineer".into();
    }
    if ml.contains("design")
        || ml.contains("environment")
        || ml.contains("blueprint")
        || ml.contains("scale")
    {
        return "architect".into();
    }
    if ml.contains("log")
        || ml.contains("metric")
        || ml.contains("trace")
        || ml.contains("root cause")
    {
        return "observability".into();
    }
    "sre".into()
}

pub fn system_prompt(agent_id: &str) -> &'static str {
    match agent_id {
        "architect" => "You are Zyra Architect, an autonomous infrastructure engineer. Design clear, reviewable infrastructure plans.",
        "devops" => "You are Zyra DevOps. Focus on CI/CD, runbooks, and safe operational changes.",
        "kubernetes" => "You are Zyra Kubernetes. Manage clusters, namespaces, and workloads with approval-gated actions.",
        "security" => "You are Zyra Security. Analyze threats, exposure, and compliance with actionable recommendations.",
        "cost" => "You are Zyra Cost Optimizer. Identify waste and savings with FinOps best practices.",
        "observability" => "You are Zyra Observability. Correlate logs, metrics, and traces for root cause analysis.",
        "ai_engineer" => "You are Zyra AI Engineer. Guide LLM deployment, vLLM, Ollama, and inference stacks.",
        "database" => "You are Zyra Database Expert. Optimize databases with clear, low-risk recommendations.",
        _ => "You are Zyra SRE, an autonomous site reliability engineer. Be concise. Use bullet points. Ground answers in host and VM reality.",
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentContext {
    pub agent_id: String,
    pub page_path: Option<String>,
    pub assembled: AssembledContext,
    pub memory_snippets: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ZyraChatBody {
    pub message: String,
    pub agent: Option<String>,
    pub vm_id: Option<Uuid>,
    pub host_id: Option<Uuid>,
    #[serde(default)]
    pub vm_ids: Option<Vec<Uuid>>,
    pub page_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ZyraChatResponse {
    pub reply: String,
    pub deterministic: bool,
    pub agent_id: String,
    pub context_summary: Option<String>,
}

pub async fn chat(
    pool: &DbPool,
    cfg: &crate::config::ControllerConfig,
    body: &ZyraChatBody,
    user_id: Option<&str>,
) -> anyhow::Result<ZyraChatResponse> {
    let agent_id = if body.agent.as_deref() == Some("auto") || body.agent.is_none() {
        pick_agent(&body.message, body.page_path.as_deref())
    } else {
        resolve_agent_id(body.agent.as_deref())
    };
    let base = super::build_copilot_base(
        pool,
        cfg,
        &body.message,
        body.vm_id,
        body.host_id,
        body.vm_ids.clone(),
    )
    .await?;
    let mut reply = base.reply;
    let task_class = TaskClass::from_agent(&agent_id);
    let memory = super::memory_store::recall_for_user(pool, user_id, 3)
        .await
        .unwrap_or_default();
    let memory_text = memory.join("\n");
    let system = system_prompt(&agent_id);
    let safe_message = body.message.chars().take(8192).collect::<String>();
    let ctx: String = base.ctx_json.chars().take(16384).collect();
    let user_prompt = format!(
        "Agent: {agent_id}\nPage: {}\n<memory>\n{memory_text}\n</memory>\nContext: {}\n<user_message>\n{safe_message}\n</user_message>",
        body.page_path.as_deref().unwrap_or(""),
        ctx,
    );
    let mut deterministic = true;
    if let Ok(Some(llm_text)) = super::llm::complete(
        pool,
        super::llm::CompletionRequest {
            task_class,
            system: system.to_string(),
            user: user_prompt,
            agent_id: Some(agent_id.clone()),
            user_id: user_id.map(String::from),
        },
    )
    .await
    {
        reply.push_str("\n\n");
        reply.push_str(&llm_text);
        deterministic = false;
    }
    Ok(ZyraChatResponse {
        reply,
        deterministic,
        agent_id,
        context_summary: Some(base.context_summary),
    })
}

pub async fn save_preference(
    pool: &DbPool,
    user_id: &str,
    agent_id: &str,
) -> anyhow::Result<()> {
    crate::db::query(
        "INSERT INTO ai_user_preferences (user_id, default_agent, updated_at)
         VALUES (?, ?, datetime('now'))
         ON CONFLICT (user_id) DO UPDATE SET default_agent = EXCLUDED.default_agent, updated_at = datetime('now')",
    )
    .bind(user_id)
    .bind(agent_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_preference(pool: &DbPool, user_id: &str) -> anyhow::Result<String> {
    let agent: Option<String> =
        crate::db::query_scalar("SELECT default_agent FROM ai_user_preferences WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(agent.unwrap_or_else(|| "auto".into()))
}
