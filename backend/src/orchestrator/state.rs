//! Deterministic workflow state. Large data lives in PostgreSQL; the state
//! holds ids and small references only.

use uuid::Uuid;

use crate::{
    error::StateError,
    models::{
        ClaimReference, Critique, ResearchQuestion, ResearchStatus, ResearchTask, SourceReference,
        VerifiedClaim,
    },
};

#[derive(Debug, Clone)]
pub struct ResearchState {
    pub research_id: Uuid,
    pub query: String,
    pub research_questions: Vec<ResearchQuestion>,
    pub tasks: Vec<ResearchTask>,
    pub sources: Vec<SourceReference>,
    pub claims: Vec<ClaimReference>,
    pub verified_claims: Vec<VerifiedClaim>,
    pub contradiction_ids: Vec<Uuid>,
    pub draft_report_id: Option<Uuid>,
    pub critique: Option<Critique>,
    /// 1-based iteration counter.
    pub iteration: u32,
    pub max_iterations: u32,
    pub status: ResearchStatus,
}

impl ResearchState {
    pub fn new(research_id: Uuid, query: String, max_iterations: u32) -> Self {
        Self {
            research_id,
            query,
            research_questions: vec![],
            tasks: vec![],
            sources: vec![],
            claims: vec![],
            verified_claims: vec![],
            contradiction_ids: vec![],
            draft_report_id: None,
            critique: None,
            iteration: 1,
            max_iterations: max_iterations.max(1),
            status: ResearchStatus::Planning,
        }
    }

    /// Whether `from -> to` is a legal forward step. The loop-back
    /// `Critiquing -> Researching` is only reachable via [`Self::begin_next_iteration`].
    pub fn is_valid_transition(from: ResearchStatus, to: ResearchStatus) -> bool {
        use ResearchStatus::*;
        if from.is_terminal() {
            return false;
        }
        matches!(
            (from, to),
            (Planning, Researching)
                | (Researching, Verifying)
                | (Verifying, Analyzing)
                | (Analyzing, Synthesizing)
                | (Synthesizing, Critiquing)
                | (Critiquing, Completed)
                | (_, Failed)
        )
    }

    pub fn transition(&mut self, to: ResearchStatus) -> Result<(), StateError> {
        if !Self::is_valid_transition(self.status, to) {
            return Err(StateError::InvalidTransition {
                from: self.status.to_string(),
                to: to.to_string(),
            });
        }
        self.status = to;
        Ok(())
    }

    pub fn can_iterate(&self) -> bool {
        self.status == ResearchStatus::Critiquing && self.iteration < self.max_iterations
    }

    /// Loops back from critique to research, never beyond the iteration limit.
    pub fn begin_next_iteration(&mut self) -> Result<(), StateError> {
        if self.status != ResearchStatus::Critiquing {
            return Err(StateError::InvalidTransition {
                from: self.status.to_string(),
                to: ResearchStatus::Researching.to_string(),
            });
        }
        if self.iteration >= self.max_iterations {
            return Err(StateError::IterationLimit(self.max_iterations));
        }
        self.iteration += 1;
        self.status = ResearchStatus::Researching;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ResearchStatus::*;

    fn state(max: u32) -> ResearchState {
        ResearchState::new(Uuid::new_v4(), "q".into(), max)
    }

    fn advance_to_critiquing(s: &mut ResearchState) {
        for next in [Researching, Verifying, Analyzing, Synthesizing, Critiquing] {
            s.transition(next).unwrap();
        }
    }

    #[test]
    fn walks_the_full_lifecycle() {
        let mut s = state(2);
        advance_to_critiquing(&mut s);
        s.transition(Completed).unwrap();
        assert!(s.status.is_terminal());
    }

    #[test]
    fn rejects_skipped_and_backward_steps() {
        let mut s = state(2);
        assert!(s.transition(Synthesizing).is_err());
        s.transition(Researching).unwrap();
        assert!(s.transition(Planning).is_err());
        assert!(s.transition(Completed).is_err());
    }

    #[test]
    fn terminal_states_are_final() {
        let mut s = state(2);
        s.transition(Failed).unwrap();
        assert!(s.transition(Researching).is_err());
        assert!(s.transition(Failed).is_err());
    }

    #[test]
    fn any_active_state_can_fail() {
        for from in [Planning, Researching, Verifying, Analyzing, Synthesizing, Critiquing] {
            assert!(ResearchState::is_valid_transition(from, Failed), "{from}");
        }
    }

    #[test]
    fn iteration_loop_is_bounded() {
        let mut s = state(2);
        advance_to_critiquing(&mut s);
        assert!(s.can_iterate());
        s.begin_next_iteration().unwrap();
        assert_eq!((s.iteration, s.status), (2, Researching));

        for next in [Verifying, Analyzing, Synthesizing, Critiquing] {
            s.transition(next).unwrap();
        }
        assert!(!s.can_iterate());
        assert!(matches!(s.begin_next_iteration(), Err(StateError::IterationLimit(2))));
        assert_eq!(s.iteration, 2);
    }

    #[test]
    fn can_only_loop_back_from_critique() {
        let mut s = state(3);
        s.transition(Researching).unwrap();
        assert!(s.begin_next_iteration().is_err());
        assert!(s.transition(Researching).is_err(), "no direct critique->research via transition()");
    }

    #[test]
    fn a_single_iteration_budget_never_loops() {
        let mut s = state(0); // clamped to 1
        advance_to_critiquing(&mut s);
        assert!(!s.can_iterate());
    }
}
