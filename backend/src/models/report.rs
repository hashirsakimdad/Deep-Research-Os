use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::string_enum;

/// A statement in the report together with the claims that back it.
/// The model only ever names claim ids; URLs are attached by Rust.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CitedStatement {
    pub text: String,
    #[serde(default)]
    pub claim_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReportSection {
    pub heading: String,
    pub body: Vec<CitedStatement>,
}

/// Structured report produced by the synthesizer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReportContent {
    pub executive_summary: String,
    pub methodology: String,
    pub key_findings: Vec<CitedStatement>,
    pub detailed_findings: Vec<ReportSection>,
    #[serde(default)]
    pub contradictory_evidence: Vec<CitedStatement>,
    pub limitations: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    pub conclusion: String,
}

impl ReportContent {
    /// Every claim id referenced anywhere in the report.
    pub fn referenced_claim_ids(&self) -> HashSet<Uuid> {
        let mut ids = HashSet::new();
        let statements = self
            .key_findings
            .iter()
            .chain(self.detailed_findings.iter().flat_map(|s| s.body.iter()))
            .chain(self.contradictory_evidence.iter());
        for s in statements {
            ids.extend(s.claim_ids.iter().copied());
        }
        ids
    }
}

/// Source entry in a rendered bibliography.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Citation {
    pub number: usize,
    pub source_id: Uuid,
    pub url: String,
    pub title: String,
}

/// Maps claims to numbered sources; numbers are assigned in order of first use.
#[derive(Debug, Default)]
pub struct CitationIndex {
    claim_source: HashMap<Uuid, (Uuid, String, String)>,
    numbers: BTreeMap<Uuid, usize>,
    order: Vec<Uuid>,
}

impl CitationIndex {
    /// `claims`: (claim id, source id, source url, source title).
    pub fn new(claims: impl IntoIterator<Item = (Uuid, Uuid, String, String)>) -> Self {
        Self { claim_source: claims.into_iter().map(|(c, s, u, t)| (c, (s, u, t))).collect(), ..Self::default() }
    }

    pub fn knows_claim(&self, claim_id: &Uuid) -> bool {
        self.claim_source.contains_key(claim_id)
    }

    /// Citation number for a claim's source, registering it on first use.
    pub fn cite(&mut self, claim_id: &Uuid) -> Option<usize> {
        let (source_id, _, _) = self.claim_source.get(claim_id)?;
        let next = self.numbers.len() + 1;
        let n = *self.numbers.entry(*source_id).or_insert(next);
        if n == next {
            self.order.push(*source_id);
        }
        Some(n)
    }

    pub fn citations(&self) -> Vec<Citation> {
        self.order
            .iter()
            .filter_map(|sid| {
                let (_, url, title) = self.claim_source.values().find(|(s, _, _)| s == sid)?;
                Some(Citation { number: self.numbers[sid], source_id: *sid, url: url.clone(), title: title.clone() })
            })
            .collect()
    }
}

fn render_statement(s: &CitedStatement, idx: &mut CitationIndex) -> String {
    let mut numbers: Vec<usize> = s.claim_ids.iter().filter_map(|c| idx.cite(c)).collect();
    numbers.sort_unstable();
    numbers.dedup();
    let marks: String = numbers.iter().map(|n| format!("[{n}]")).collect();
    if marks.is_empty() { s.text.trim().to_string() } else { format!("{} {}", s.text.trim(), marks) }
}

/// Renders markdown. Citation numbers and the bibliography come exclusively
/// from the claim -> source mapping in `idx`, never from model output.
pub fn render_markdown(query: &str, content: &ReportContent, idx: &mut CitationIndex) -> (String, Vec<Citation>) {
    let mut md = String::new();
    md.push_str("# Research Report\n\n");
    md.push_str(&format!("## Research Question\n\n{}\n\n", query.trim()));
    md.push_str(&format!("## Executive Summary\n\n{}\n\n", content.executive_summary.trim()));
    md.push_str(&format!("## Methodology\n\n{}\n\n", content.methodology.trim()));

    md.push_str("## Key Findings\n\n");
    for f in &content.key_findings {
        md.push_str(&format!("- {}\n", render_statement(f, idx)));
    }
    md.push_str("\n## Detailed Findings\n\n");
    for section in &content.detailed_findings {
        md.push_str(&format!("### {}\n\n", section.heading.trim()));
        for s in &section.body {
            md.push_str(&format!("{}\n\n", render_statement(s, idx)));
        }
    }
    md.push_str("## Contradictory Evidence\n\n");
    if content.contradictory_evidence.is_empty() {
        md.push_str("No genuine contradictions between sources were identified.\n\n");
    } else {
        for s in &content.contradictory_evidence {
            md.push_str(&format!("- {}\n", render_statement(s, idx)));
        }
        md.push('\n');
    }
    md.push_str("## Limitations\n\n");
    for l in &content.limitations {
        md.push_str(&format!("- {}\n", l.trim()));
    }
    md.push_str("\n## Open Questions\n\n");
    for q in &content.open_questions {
        md.push_str(&format!("- {}\n", q.trim()));
    }
    md.push_str(&format!("\n## Conclusion\n\n{}\n\n", content.conclusion.trim()));

    let citations = idx.citations();
    md.push_str("## Sources\n\n");
    for c in &citations {
        md.push_str(&format!("[{}] {} - {}\n\n", c.number, c.title.trim(), c.url));
    }
    (md, citations)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub id: Uuid,
    pub research_run_id: Uuid,
    pub iteration: i32,
    pub is_final: bool,
    pub content: ReportContent,
    pub markdown: String,
    pub citations: Vec<Citation>,
    pub created_at: DateTime<Utc>,
}

string_enum! {
    pub enum IssueType {
        UnsupportedClaim => "UNSUPPORTED_CLAIM",
        InsufficientEvidence => "INSUFFICIENT_EVIDENCE",
        MissingCitation => "MISSING_CITATION",
        LogicalInconsistency => "LOGICAL_INCONSISTENCY",
        Overconfident => "OVERCONFIDENT",
        IgnoredContradiction => "IGNORED_CONTRADICTION",
        UnansweredQuestion => "UNANSWERED_QUESTION",
        SourceQuality => "SOURCE_QUALITY",
    }
}

string_enum! {
    pub enum Severity {
        High => "high",
        Medium => "medium",
        Low => "low",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CritiqueIssue {
    #[serde(rename = "type")]
    pub kind: IssueType,
    #[serde(default)]
    pub claim_id: Option<Uuid>,
    pub description: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Critique {
    pub id: Uuid,
    pub research_run_id: Uuid,
    pub report_id: Uuid,
    pub iteration: i32,
    pub quality: f32,
    pub needs_more_research: bool,
    pub issues: Vec<CritiqueIssue>,
    pub required_research: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stmt(text: &str, ids: &[Uuid]) -> CitedStatement {
        CitedStatement { text: text.into(), claim_ids: ids.to_vec() }
    }

    fn content(finding: CitedStatement) -> ReportContent {
        ReportContent {
            executive_summary: "sum".into(),
            methodology: "meth".into(),
            key_findings: vec![finding],
            detailed_findings: vec![],
            contradictory_evidence: vec![],
            limitations: vec!["lim".into()],
            open_questions: vec![],
            conclusion: "done".into(),
        }
    }

    #[test]
    fn citations_are_numbered_by_first_use_and_deduplicated_by_source() {
        let (c1, c2, c3) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (s1, s2) = (Uuid::new_v4(), Uuid::new_v4());
        let mut idx = CitationIndex::new([
            (c1, s1, "https://a.example/x".into(), "A".into()),
            (c2, s1, "https://a.example/x".into(), "A".into()),
            (c3, s2, "https://b.example/y".into(), "B".into()),
        ]);
        let report = content(stmt("Finding", &[c3, c1, c2]));
        let (md, cites) = render_markdown("q", &report, &mut idx);
        assert_eq!(cites.len(), 2);
        assert_eq!(cites[0].url, "https://b.example/y");
        assert_eq!(cites[1].url, "https://a.example/x");
        assert!(md.contains("Finding [1][2]"));
        assert!(md.contains("[2] A - https://a.example/x"));
    }

    #[test]
    fn unknown_claim_ids_never_produce_citations() {
        let mut idx = CitationIndex::new([]);
        let report = content(stmt("Uncited", &[Uuid::new_v4()]));
        let (md, cites) = render_markdown("q", &report, &mut idx);
        assert!(cites.is_empty());
        assert!(md.contains("- Uncited\n"));
    }

    #[test]
    fn collects_referenced_claim_ids() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut r = content(stmt("x", &[a]));
        r.contradictory_evidence.push(stmt("y", &[b]));
        assert_eq!(r.referenced_claim_ids(), HashSet::from([a, b]));
    }
}
