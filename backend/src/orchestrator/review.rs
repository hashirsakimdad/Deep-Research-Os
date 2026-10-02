//! Explicit, deterministic criteria for what happens after the critic speaks.
//! The critic's numeric score is one input, never the sole decision maker.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::models::{
    Contradiction, CritiqueIssue, IssueType, ReportContent, Severity, SupportLevel,
};

/// Reports scoring below this are never accepted without caveats.
pub const MIN_ACCEPTABLE_QUALITY: f32 = 0.7;

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewDecision {
    /// Good enough as is.
    Accept,
    /// Run another research iteration on these follow-up questions.
    Iterate { follow_ups: Vec<FollowUp> },
    /// Out of iterations (or nothing new to research) but problems remain;
    /// finish the report with the outstanding issues stated as limitations.
    AcceptWithLimitations,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FollowUp {
    pub question: String,
    /// Original research question this follow-up serves, if known.
    pub question_id: Option<Uuid>,
}

pub struct ReviewInput<'a> {
    pub quality: f32,
    pub critic_wants_more: bool,
    pub required_research: &'a [String],
    pub issues: &'a [CritiqueIssue],
    /// Research questions with no citable claims: (id, text).
    pub uncovered_questions: &'a [(Uuid, String)],
    pub iteration: u32,
    pub max_iterations: u32,
    pub max_follow_ups: usize,
}

pub fn has_blocking_issue(issues: &[CritiqueIssue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::High)
}

pub fn decide(input: &ReviewInput<'_>) -> ReviewDecision {
    let blocked = has_blocking_issue(input.issues) || input.quality < MIN_ACCEPTABLE_QUALITY;

    let mut follow_ups: Vec<FollowUp> = Vec::new();
    let mut seen = HashSet::new();
    // Deterministic gaps first: they are facts, not opinions.
    for (id, q) in input.uncovered_questions {
        if seen.insert(q.to_lowercase()) {
            follow_ups.push(FollowUp { question: q.clone(), question_id: Some(*id) });
        }
    }
    if input.critic_wants_more {
        for q in input.required_research {
            let q = q.trim();
            if !q.is_empty() && seen.insert(q.to_lowercase()) {
                follow_ups.push(FollowUp { question: q.to_string(), question_id: None });
            }
        }
    }
    follow_ups.truncate(input.max_follow_ups);

    let iterations_left = input.iteration < input.max_iterations;
    if !follow_ups.is_empty() && iterations_left {
        ReviewDecision::Iterate { follow_ups }
    } else if blocked || !follow_ups.is_empty() {
        ReviewDecision::AcceptWithLimitations
    } else {
        ReviewDecision::Accept
    }
}

/// Problems that can be established without asking a model.
pub fn deterministic_issues(
    report: &ReportContent,
    support: &HashMap<Uuid, SupportLevel>,
    contradictions: &[Contradiction],
) -> Vec<CritiqueIssue> {
    let mut issues = Vec::new();

    for f in &report.key_findings {
        if f.claim_ids.is_empty() {
            issues.push(CritiqueIssue {
                kind: IssueType::MissingCitation,
                claim_id: None,
                description: format!("Key finding has no supporting claims: \"{}\"", f.text.trim()),
                severity: Severity::High,
            });
        } else if f.claim_ids.iter().all(|c| support.get(c) != Some(&SupportLevel::Supported)) {
            issues.push(CritiqueIssue {
                kind: IssueType::InsufficientEvidence,
                claim_id: f.claim_ids.first().copied(),
                description: format!(
                    "Key finding rests only on partially supported claims; its wording must stay within their scope: \"{}\"",
                    f.text.trim()
                ),
                severity: Severity::Low,
            });
        }
    }

    let discussed = &report
        .contradictory_evidence
        .iter()
        .flat_map(|s| s.claim_ids.iter().copied())
        .collect::<HashSet<_>>();
    for c in contradictions.iter().filter(|c| c.kind.is_genuine()) {
        if !discussed.contains(&c.claim_a_id) && !discussed.contains(&c.claim_b_id) {
            issues.push(CritiqueIssue {
                kind: IssueType::IgnoredContradiction,
                claim_id: Some(c.claim_a_id),
                description: format!("A genuine contradiction is not discussed: {}", c.explanation),
                severity: Severity::Medium,
            });
        }
    }
    issues
}

/// Research questions that have no citable claim at all.
pub fn uncovered_questions(
    questions: &[(Uuid, String)],
    claims: impl IntoIterator<Item = (Option<Uuid>, SupportLevel)>,
) -> Vec<(Uuid, String)> {
    let covered: HashSet<Uuid> = claims
        .into_iter()
        .filter(|(_, s)| s.is_citable())
        .filter_map(|(q, _)| q)
        .collect();
    questions.iter().filter(|(id, _)| !covered.contains(id)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CitedStatement, ContradictionKind};

    fn issue(sev: Severity) -> CritiqueIssue {
        CritiqueIssue { kind: IssueType::Overconfident, claim_id: None, description: "d".into(), severity: sev }
    }

    fn base<'a>(issues: &'a [CritiqueIssue], required: &'a [String], uncovered: &'a [(Uuid, String)]) -> ReviewInput<'a> {
        ReviewInput {
            quality: 0.85,
            critic_wants_more: false,
            required_research: required,
            issues,
            uncovered_questions: uncovered,
            iteration: 1,
            max_iterations: 2,
            max_follow_ups: 4,
        }
    }

    #[test]
    fn clean_report_is_accepted() {
        assert_eq!(decide(&base(&[issue(Severity::Low)], &[], &[])), ReviewDecision::Accept);
    }

    #[test]
    fn critic_follow_ups_trigger_an_iteration_while_budget_remains() {
        let req = vec!["Find replication studies".to_string(), "find replication studies".to_string()];
        let mut i = base(&[], &req, &[]);
        i.critic_wants_more = true;
        match decide(&i) {
            ReviewDecision::Iterate { follow_ups } => assert_eq!(follow_ups.len(), 1, "duplicates collapse"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn follow_ups_are_ignored_unless_the_critic_asks_for_research() {
        let req = vec!["something".to_string()];
        assert_eq!(decide(&base(&[], &req, &[])), ReviewDecision::Accept);
    }

    #[test]
    fn uncovered_questions_force_iteration_even_if_the_critic_is_happy() {
        let gap = vec![(Uuid::new_v4(), "What about security?".to_string())];
        match decide(&base(&[], &[], &gap)) {
            ReviewDecision::Iterate { follow_ups } => assert_eq!(follow_ups[0].question_id, Some(gap[0].0)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn last_iteration_never_loops_and_surfaces_problems() {
        let req = vec!["more".to_string()];
        let mut i = base(&[], &req, &[]);
        i.critic_wants_more = true;
        i.iteration = 2;
        assert_eq!(decide(&i), ReviewDecision::AcceptWithLimitations);
    }

    #[test]
    fn high_severity_or_low_quality_blocks_clean_acceptance() {
        let issues = [issue(Severity::High)];
        assert_eq!(decide(&base(&issues, &[], &[])), ReviewDecision::AcceptWithLimitations);
        let mut i = base(&[], &[], &[]);
        i.quality = 0.4;
        assert_eq!(decide(&i), ReviewDecision::AcceptWithLimitations);
    }

    #[test]
    fn follow_ups_are_capped() {
        let gaps: Vec<_> = (0..10).map(|n| (Uuid::new_v4(), format!("q{n}"))).collect();
        match decide(&base(&[], &[], &gaps)) {
            ReviewDecision::Iterate { follow_ups } => assert_eq!(follow_ups.len(), 4),
            other => panic!("{other:?}"),
        }
    }

    fn report(findings: Vec<CitedStatement>, contra: Vec<CitedStatement>) -> ReportContent {
        ReportContent {
            executive_summary: "s".into(),
            methodology: "m".into(),
            key_findings: findings,
            detailed_findings: vec![],
            contradictory_evidence: contra,
            limitations: vec![],
            open_questions: vec![],
            conclusion: "c".into(),
        }
    }

    #[test]
    fn flags_uncited_findings_and_ignored_contradictions() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let r = report(vec![CitedStatement { text: "bold claim".into(), claim_ids: vec![] }], vec![]);
        let c = Contradiction {
            id: Uuid::new_v4(), research_run_id: Uuid::new_v4(), claim_a_id: a, claim_b_id: b,
            kind: ContradictionKind::DirectContradiction, explanation: "x".into(), created_at: chrono::Utc::now(),
        };
        let issues = deterministic_issues(&r, &HashMap::new(), &[c.clone()]);
        assert!(issues.iter().any(|i| i.kind == IssueType::MissingCitation && i.severity == Severity::High));
        assert!(issues.iter().any(|i| i.kind == IssueType::IgnoredContradiction));

        // Discussing either side of the contradiction satisfies the check;
        // metric differences are not "genuine" and need no discussion.
        let ok = report(
            vec![CitedStatement { text: "f".into(), claim_ids: vec![a] }],
            vec![CitedStatement { text: "conflict".into(), claim_ids: vec![a, b] }],
        );
        let support = HashMap::from([(a, SupportLevel::Supported)]);
        assert!(deterministic_issues(&ok, &support, &[c.clone()]).is_empty());
        let mut metric = c;
        metric.kind = ContradictionKind::MetricDifference;
        assert!(deterministic_issues(&report(vec![], vec![]), &HashMap::new(), &[metric]).is_empty());
    }

    #[test]
    fn partial_only_findings_get_a_low_severity_note() {
        let a = Uuid::new_v4();
        let r = report(vec![CitedStatement { text: "f".into(), claim_ids: vec![a] }], vec![]);
        let support = HashMap::from([(a, SupportLevel::PartiallySupported)]);
        let issues = deterministic_issues(&r, &support, &[]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Low);
    }

    #[test]
    fn finds_questions_without_citable_claims() {
        let (q1, q2, q3) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let qs = vec![(q1, "one".to_string()), (q2, "two".to_string()), (q3, "three".to_string())];
        let claims = vec![
            (Some(q1), SupportLevel::Supported),
            (Some(q2), SupportLevel::Unverified),
            (Some(q3), SupportLevel::PartiallySupported),
            (None, SupportLevel::Supported),
        ];
        let gaps = uncovered_questions(&qs, claims);
        assert_eq!(gaps, vec![(q2, "two".to_string())]);
    }
}
