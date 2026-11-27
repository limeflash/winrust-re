//! The search of the sidebar, as the mockup's Settings.html does it (around its line 622): every line whose label, value or section
//! title contains the query (case-insensitive), grouped by page, at most 6 for a page; a page whose own title matches and that has no
//! such line shows itself as a "Page" line. A pure function of the pages, so it is tested without a window.
use crate::Page;

/// The most lines one page shows in the results.
pub const PER_PAGE: usize = 6;

/// One line of the results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub page: usize,
    /// The key of the line to flash on the page (None: the page itself).
    pub key: Option<String>,
    pub label: String,
    /// "Page" for a page, else "Page › Section".
    pub sub: String,
}

/// The hits of one page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub page: usize,
    pub title: String,
    pub hits: Vec<Hit>,
}

/// What `search` shows for a page that matched by its title only.
#[must_use]
pub fn page_word() -> &'static str {
    t!("Страница", "Page")
}

/// The pages' lines that match `query`, page by page in the sidebar's order. Nothing for an empty query.
#[must_use]
pub fn search(pages: &[Page], query: &str) -> Vec<Found> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, pg) in pages.iter().enumerate() {
        let mut hits = Vec::new();
        for g in &pg.groups {
            for l in &g.rows {
                let hay = format!("{} {} {} {}", l.row.label(), l.sub, l.val, g.title).to_lowercase();
                if hay.contains(&q) {
                    let sub = if g.title.is_empty() { pg.title.clone() } else { format!("{} \u{203a} {}", pg.title, g.title) };
                    hits.push(Hit { page: i, key: Some(l.key.clone()), label: l.row.label().to_string(), sub });
                }
            }
        }
        if hits.is_empty() && pg.title.to_lowercase().contains(&q) {
            hits.push(Hit { page: i, key: None, label: pg.title.clone(), sub: page_word().to_string() });
        }
        hits.truncate(PER_PAGE);
        if !hits.is_empty() {
            out.push(Found { page: i, title: pg.title.clone(), hits });
        }
    }
    out
}
