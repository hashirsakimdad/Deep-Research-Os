use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::string_enum;

string_enum! {
    pub enum SourceType {
        AcademicPaper => "academic_paper",
        GovernmentDocument => "government_document",
        OfficialDocument => "official_document",
        CompanyDocument => "company_document",
        News => "news",
        ResearchReport => "research_report",
        TechnicalBlog => "technical_blog",
        Other => "other",
    }
}

string_enum! {
    /// How close a source is to the underlying evidence.
    pub enum SourceNature {
        Primary => "primary",
        Secondary => "secondary",
        Commentary => "commentary",
        Opinion => "opinion",
        Marketing => "marketing",
        Unknown => "unknown",
    }
}

impl SourceNature {
    /// Highest confidence a claim sourced from this kind of page may carry.
    pub fn confidence_cap(&self) -> f32 {
        match self {
            Self::Primary => 1.0,
            Self::Secondary => 0.85,
            Self::Unknown => 0.7,
            Self::Commentary => 0.6,
            Self::Opinion | Self::Marketing => 0.5,
        }
    }
}

impl SourceType {
    /// Cheap, deterministic guess from the URL; the extractor may refine it.
    pub fn guess_from_host(host: &str) -> Self {
        let h = host.to_lowercase();
        if h.ends_with(".gov") || h.contains(".gov.") || h.ends_with(".mil") || h.ends_with(".europa.eu") {
            Self::GovernmentDocument
        } else if h.ends_with(".edu")
            || h.contains(".ac.")
            || ["arxiv.org", "nature.com", "science.org", "acm.org", "ieee.org", "sciencedirect.com", "springer.com", "nber.org", "ssrn.com", "semanticscholar.org"]
                .iter()
                .any(|d| h == *d || h.ends_with(&format!(".{d}")))
        {
            Self::AcademicPaper
        } else if ["reuters.com", "apnews.com", "bbc.com", "nytimes.com", "wsj.com", "ft.com", "bloomberg.com", "theguardian.com", "techcrunch.com", "wired.com", "arstechnica.com"]
            .iter()
            .any(|d| h == *d || h.ends_with(&format!(".{d}")))
        {
            Self::News
        } else if ["medium.com", "substack.com", "dev.to", "github.io"]
            .iter()
            .any(|d| h == *d || h.ends_with(&format!(".{d}")))
        {
            Self::TechnicalBlog
        } else {
            Self::Other
        }
    }
}

/// A web document retrieved during research. Original URL is always kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: Uuid,
    pub research_run_id: Uuid,
    pub url: String,
    pub normalized_url: String,
    pub title: String,
    pub publisher: Option<String>,
    pub published_at: Option<String>,
    pub retrieved_at: DateTime<Utc>,
    pub content: String,
    pub content_hash: String,
    pub source_type: SourceType,
    pub nature: SourceNature,
}

/// Lightweight reference used in in-memory state and API listings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceReference {
    pub id: Uuid,
    pub url: String,
    pub title: String,
    pub publisher: Option<String>,
    pub published_at: Option<String>,
    pub source_type: SourceType,
    pub nature: SourceNature,
}

/// One hit returned by a search provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: String,
    pub published_at: Option<String>,
    /// Page text when the provider already returns it (avoids a fetch).
    pub content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_source_type_from_host() {
        assert_eq!(SourceType::guess_from_host("www.nist.gov"), SourceType::GovernmentDocument);
        assert_eq!(SourceType::guess_from_host("arxiv.org"), SourceType::AcademicPaper);
        assert_eq!(SourceType::guess_from_host("cs.stanford.edu"), SourceType::AcademicPaper);
        assert_eq!(SourceType::guess_from_host("www.reuters.com"), SourceType::News);
        assert_eq!(SourceType::guess_from_host("example.com"), SourceType::Other);
    }

    #[test]
    fn weaker_sources_cap_confidence() {
        assert!(SourceNature::Marketing.confidence_cap() < SourceNature::Secondary.confidence_cap());
        assert_eq!(SourceNature::Primary.confidence_cap(), 1.0);
    }
}
