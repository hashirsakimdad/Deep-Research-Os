//! Agents: typed components with distinct responsibilities.
//!
//! | Agent                  | Input                       | Output                    |
//! |------------------------|-----------------------------|---------------------------|
//! | [`PlannerAgent`]       | user query                  | research plan             |
//! | [`ResearcherAgent`]    | one research task           | sources + extracted claims|
//! | [`VerifierAgent`]      | claims + excerpts           | support level per claim   |
//! | [`ContradictionAgent`] | verified claims by question | classified disagreements  |
//! | [`SynthesizerAgent`]   | verified claims, conflicts  | structured report         |
//! | [`CriticAgent`]        | report + evidence summary   | critique                  |

pub mod agent;
pub mod contradiction;
pub mod critic;
pub mod planner;
pub mod researcher;
pub mod synthesizer;
pub mod verifier;

pub use agent::{Agent, EventSink, NoopSink};
pub use contradiction::ContradictionAgent;
pub use critic::CriticAgent;
pub use planner::PlannerAgent;
pub use researcher::ResearcherAgent;
pub use synthesizer::SynthesizerAgent;
pub use verifier::VerifierAgent;
