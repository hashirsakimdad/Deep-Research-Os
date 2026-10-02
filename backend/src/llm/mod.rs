//! LLM access. Agents depend only on [`LlmProvider`]; Grok is one implementation.

pub mod grok;
pub mod provider;
pub mod structured;

pub use grok::GrokProvider;
pub use provider::{
    LimitedProvider, LlmMessage, LlmProvider, LlmRequest, LlmResponse, MessageRole, Usage,
};
pub use structured::generate_structured;
