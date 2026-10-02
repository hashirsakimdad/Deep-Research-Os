//! External tools used by the research agent: web search and page fetching.

pub mod fetch;
pub mod search;

pub use fetch::{FetchedPage, Fetcher, HttpFetcher};
pub use search::{SearchProvider, TavilySearch};
