use anyhow::Result;
use futures_util::StreamExt;
use serde::Serialize;
use std::pin::Pin;
use std::sync::Arc;
use futures_util::Stream;

use crate::{
    agents::{hermes::HermesClient, openclaw::{ChatMessage, OpenClawClient}},
    config::Config,
    db::models::Message,
};

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "chunk")]
    Chunk { agent: String, content: String },
    #[serde(rename = "done")]
    Done { agent: String },
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum AgentMode {
    HermesOnly,
    OpenClawOnly,
    Debate,
}

fn messages_to_chat(history: &[Message]) -> Vec<ChatMessage> {
    history.iter().map(|m| ChatMessage {
        role: if m.role == "user" { "user".into() } else { "assistant".into() },
        content: m.content.clone(),
    }).collect()
}

pub async fn run_agent_turn(
    config: &Arc<Config>,
    history: &[Message],
    user_message: &str,
    mode: AgentMode,
) -> Result<Vec<(String, String, Option<String>)>> {
    let mut chat = messages_to_chat(history);
    chat.push(ChatMessage { role: "user".into(), content: user_message.into() });

    let mut results = vec![];
    match mode {
        AgentMode::OpenClawOnly => {
            let reply = OpenClawClient::new(config).chat(chat).await?;
            results.push(("openclaw".into(), reply, Some("OpenClaw".into())));
        }
        AgentMode::HermesOnly => {
            let reply = HermesClient::new(config).chat(chat).await?;
            results.push(("hermes".into(), reply, Some("Hermes".into())));
        }
        AgentMode::Debate => {
            let openclaw = OpenClawClient::new(config);
            let hermes = HermesClient::new(config);

            let oc_reply = openclaw.chat(chat.clone()).await?;
            results.push(("openclaw".into(), oc_reply.clone(), Some("OpenClaw".into())));

            let mut h_ctx = chat.clone();
            h_ctx.push(ChatMessage { role: "assistant".into(), content: format!("[OpenClaw]: {}", oc_reply) });
            h_ctx.push(ChatMessage { role: "user".into(), content: "Review OpenClaw's response and provide architectural perspective, corrections, or alternatives.".into() });
            let h_reply = hermes.chat(h_ctx).await?;
            results.push(("hermes".into(), h_reply.clone(), Some("Hermes".into())));

            let mut oc_final = chat;
            oc_final.push(ChatMessage { role: "assistant".into(), content: format!("[OpenClaw]: {}", oc_reply) });
            oc_final.push(ChatMessage { role: "assistant".into(), content: format!("[Hermes review]: {}", h_reply) });
            oc_final.push(ChatMessage { role: "user".into(), content: "Based on Hermes' review, refine or defend your implementation with concrete code.".into() });
            let oc_final_reply = openclaw.chat(oc_final).await?;
            results.push(("openclaw".into(), oc_final_reply, Some("OpenClaw".into())));
        }
    }
    Ok(results)
}

pub fn run_agent_stream(
    config: &Arc<Config>,
    history: &[Message],
    user_message: &str,
    mode: AgentMode,
) -> Pin<Box<dyn Stream<Item = ServerEvent> + Send>> {
    let config = config.clone();
    let mut chat = messages_to_chat(history);
    chat.push(ChatMessage { role: "user".into(), content: user_message.to_string() });

    Box::pin(async_stream::stream! {
        match mode {
            AgentMode::OpenClawOnly => {
                let mut stream = OpenClawClient::new(&config).chat_stream(chat);
                while let Some(chunk) = stream.next().await {
                    yield ServerEvent::Chunk { agent: "OpenClaw".into(), content: chunk };
                }
                yield ServerEvent::Done { agent: "OpenClaw".into() };
            }
            AgentMode::HermesOnly => {
                let mut stream = HermesClient::new(&config).chat_stream(chat);
                while let Some(chunk) = stream.next().await {
                    yield ServerEvent::Chunk { agent: "Hermes".into(), content: chunk };
                }
                yield ServerEvent::Done { agent: "Hermes".into() };
            }
            AgentMode::Debate => {
                let openclaw = OpenClawClient::new(&config);
                let hermes = HermesClient::new(&config);

                // Round 1: OpenClaw
                let mut oc_buf = String::new();
                let mut oc_s = openclaw.chat_stream(chat.clone());
                while let Some(c) = oc_s.next().await {
                    oc_buf.push_str(&c);
                    yield ServerEvent::Chunk { agent: "OpenClaw".into(), content: c };
                }
                yield ServerEvent::Done { agent: "OpenClaw".into() };
                drop(oc_s);

                // Round 2: Hermes reviews
                let mut h_ctx = chat.clone();
                h_ctx.push(ChatMessage { role: "assistant".into(), content: format!("[OpenClaw]: {}", oc_buf) });
                h_ctx.push(ChatMessage { role: "user".into(), content: "Review OpenClaw's response and provide architectural feedback.".into() });
                let mut h_buf = String::new();
                let mut h_s = hermes.chat_stream(h_ctx);
                while let Some(c) = h_s.next().await {
                    h_buf.push_str(&c);
                    yield ServerEvent::Chunk { agent: "Hermes".into(), content: c };
                }
                yield ServerEvent::Done { agent: "Hermes".into() };
                drop(h_s);

                // Round 3: OpenClaw refines
                let mut oc_final_ctx = chat;
                oc_final_ctx.push(ChatMessage { role: "assistant".into(), content: format!("[OpenClaw]: {}", oc_buf) });
                oc_final_ctx.push(ChatMessage { role: "assistant".into(), content: format!("[Hermes review]: {}", h_buf) });
                oc_final_ctx.push(ChatMessage { role: "user".into(), content: "Refine your implementation based on the architectural review.".into() });
                let mut oc_final_s = openclaw.chat_stream(oc_final_ctx);
                while let Some(c) = oc_final_s.next().await {
                    yield ServerEvent::Chunk { agent: "OpenClaw".into(), content: c };
                }
                yield ServerEvent::Done { agent: "OpenClaw".into() };
            }
        }
    })
}
