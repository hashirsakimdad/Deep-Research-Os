use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;

use crate::{
    error::FetchError,
    util::retry::{RetryPolicy, retry},
};

/// Maximum body size read from a page.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// Pages with less extracted text than this are treated as empty.
const MIN_TEXT_CHARS: usize = 200;

#[derive(Debug, Clone)]
pub struct FetchedPage {
    pub title: Option<String>,
    pub text: String,
}

/// Page retrieval abstraction.
#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<FetchedPage, FetchError>;
}

pub struct HttpFetcher {
    client: Client,
    retry: RetryPolicy,
}

impl HttpFetcher {
    pub fn new(timeout: Duration) -> Result<Self, FetchError> {
        let client = Client::builder()
            .timeout(timeout)
            .user_agent("DeepResearchOS/0.1 (+research bot)")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        Ok(Self { client, retry: RetryPolicy::new(2, Duration::from_millis(400), Duration::from_secs(2)) })
    }

    async fn attempt(&self, url: &str) -> Result<FetchedPage, FetchError> {
        let mut resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| if e.is_timeout() { FetchError::Timeout } else { FetchError::Network(e.to_string()) })?;
        if !resp.status().is_success() {
            return Err(FetchError::Status(resp.status().as_u16()));
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        let is_html = content_type.contains("html") || content_type.is_empty();
        if !is_html && !content_type.starts_with("text/") {
            return Err(FetchError::UnsupportedContent(content_type));
        }

        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| FetchError::Network(e.to_string()))? {
            body.extend_from_slice(&chunk);
            if body.len() >= MAX_BODY_BYTES {
                body.truncate(MAX_BODY_BYTES);
                break;
            }
        }
        let raw = String::from_utf8_lossy(&body);
        let page = if is_html { html_to_text(&raw) } else { FetchedPage { title: None, text: collapse_ws(&raw) } };
        if page.text.chars().count() < MIN_TEXT_CHARS {
            return Err(FetchError::Empty);
        }
        Ok(page)
    }
}

#[async_trait]
impl Fetcher for HttpFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchedPage, FetchError> {
        retry(self.retry, FetchError::is_retryable, |_| self.attempt(url)).await
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Minimal HTML-to-text conversion: drops script/style/navigation blocks and
/// tags, keeps paragraph boundaries, decodes common entities.
pub fn html_to_text(html: &str) -> FetchedPage {
    const SKIP: [&str; 9] = ["script", "style", "noscript", "nav", "footer", "header", "svg", "title", "head"];
    const BLOCK: [&str; 14] =
        ["p", "div", "br", "li", "ul", "ol", "h1", "h2", "h3", "h4", "tr", "section", "article", "blockquote"];

    let lower = html.to_lowercase();
    let title = find_between(html, &lower, "<title", "</title>")
        .map(|t| collapse_ws(&decode_entities(t)))
        .filter(|t| !t.is_empty());

    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len() / 3);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            // copy a run of text up to the next tag
            let next = html[i..].find('<').map_or(html.len(), |p| i + p);
            out.push_str(&html[i..next]);
            i = next;
            continue;
        }
        let Some(close) = html[i..].find('>').map(|p| i + p) else { break };
        let tag_src = &lower[i + 1..close];
        let name: String = tag_src.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        let is_closing = tag_src.starts_with('/');
        i = close + 1;
        if !is_closing && SKIP.contains(&name.as_str()) {
            let end_tag = format!("</{name}");
            i = lower[i..].find(&end_tag).map_or(html.len(), |p| i + p);
            i = html[i..].find('>').map_or(html.len(), |p| i + p + 1);
        } else if html[..close].ends_with("--") && tag_src.starts_with("!--") {
            // comment: skip to terminator
        } else if BLOCK.contains(&name.as_str()) {
            out.push('\n');
        }
    }

    let text = decode_entities(&out).lines().map(collapse_ws).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    FetchedPage { title, text }
}

fn find_between<'a>(orig: &'a str, lower: &str, open: &str, close: &str) -> Option<&'a str> {
    let start = lower.find(open)?;
    let body_start = lower[start..].find('>')? + start + 1;
    let end = lower[body_start..].find(close)? + body_start;
    Some(&orig[body_start..end])
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&rsquo;", "'")
        .replace("&ldquo;", "\"")
        .replace("&rdquo;", "\"")
}

#[cfg(test)]
pub(crate) mod testing {
    use std::{collections::HashMap, sync::Mutex};

    use super::*;

    /// Serves canned pages by URL; unknown URLs fail.
    #[derive(Default)]
    pub struct MockFetcher {
        pub pages: Mutex<HashMap<String, FetchedPage>>,
        pub calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Fetcher for MockFetcher {
        async fn fetch(&self, url: &str) -> Result<FetchedPage, FetchError> {
            self.calls.lock().unwrap().push(url.to_string());
            self.pages.lock().unwrap().get(url).cloned().ok_or(FetchError::Status(404))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    #[test]
    fn extracts_title_and_visible_text() {
        let html = r#"<html><head><title>My &amp; Title</title><style>p{color:red}</style></head>
            <body><nav>Home | About</nav><script>var x = "<p>no</p>";</script>
            <article><h1>Heading</h1><p>First   paragraph &quot;quoted&quot;.</p><p>Second.</p></article>
            <footer>copyright</footer></body></html>"#;
        let page = html_to_text(html);
        assert_eq!(page.title.as_deref(), Some("My & Title"));
        assert_eq!(page.text, "Heading\nFirst paragraph \"quoted\".\nSecond.");
    }

    #[tokio::test]
    async fn fetches_html_and_rejects_binary() {
        let server = MockServer::start().await;
        let body = format!("<html><title>T</title><body><p>{}</p></body></html>", "word ".repeat(100));
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/html"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/file.pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(vec![1u8, 2, 3], "application/pdf"))
            .mount(&server)
            .await;
        let fetcher = HttpFetcher::new(Duration::from_secs(5)).unwrap();
        let page = fetcher.fetch(&format!("{}/page", server.uri())).await.unwrap();
        assert_eq!(page.title.as_deref(), Some("T"));
        assert!(page.text.starts_with("word word"));
        let err = fetcher.fetch(&format!("{}/file.pdf", server.uri())).await.unwrap_err();
        assert!(matches!(err, FetchError::UnsupportedContent(_)));
    }

    #[tokio::test]
    async fn short_pages_are_empty() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_raw("<p>hi</p>", "text/html"))
            .mount(&server)
            .await;
        let fetcher = HttpFetcher::new(Duration::from_secs(5)).unwrap();
        assert!(matches!(fetcher.fetch(&server.uri()).await, Err(FetchError::Empty)));
    }
}
