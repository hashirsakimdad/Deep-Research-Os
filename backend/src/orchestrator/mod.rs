//! The deterministic heart of the system. Rust decides what runs, when, how
//! often and with which limits; models only provide reasoning inside agents.

pub mod events;
pub mod review;
pub mod scheduler;
pub mod state;
pub mod workflow;

pub use events::EventBus;
pub use state::ResearchState;
pub use workflow::Orchestrator;
