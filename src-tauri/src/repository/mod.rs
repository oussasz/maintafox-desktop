pub mod equipment_repository;
pub mod lookup_repository;
pub mod org_repository;
pub mod team_repository;

use serde::{Deserialize, Serialize};

// ── Pagination ────────────────────────────────────────────────────────────

/// Standard pagination parameters accepted by all list queries.
#[derive(Debug, Clone, Deserialize)]
pub struct PageRequest {
    /// 1-based page number
    pub page: u64,
    /// Items per page (capped at 200 by the repository layer)
    pub per_page: u64,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self { page: 1, per_page: 50 }
    }
}

impl PageRequest {
    /// The maximum `per_page` the application will honor.
    /// Prevents runaway queries from a malformed caller.
    pub const MAX_PER_PAGE: u64 = 200;

    /// Offset for SQL LIMIT/OFFSET queries.
    pub fn offset(&self) -> u64 {
        let pp = self.per_page.min(Self::MAX_PER_PAGE);
        (self.page.saturating_sub(1)) * pp
    }

    /// Capped limit for SQL LIMIT/OFFSET queries.
    pub fn limit(&self) -> u64 {
        self.per_page.min(Self::MAX_PER_PAGE)
    }
}

/// Standard paginated response wrapper.
#[derive(Debug, Clone, Serialize)]
pub struct Page<T: Serialize> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
    pub total_pages: u64,
}

impl<T: Serialize> Page<T> {
    pub fn new(items: Vec<T>, total: u64, request: &PageRequest) -> Self {
        let per_page = request.per_page.min(PageRequest::MAX_PER_PAGE);
        let total_pages = if per_page == 0 {
            0
        } else {
            (total + per_page - 1) / per_page
        };
        Self {
            items,
            total,
            page: request.page,
            per_page,
            total_pages,
        }
    }
}

// ── Sort ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

// ── Common filter fields (shared across repos) ────────────────────────────

/// Standard text search filter parameter.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SearchFilter {
    /// Optional free-text search string. Applied to name/label/code columns.
    pub query: Option<String>,
    /// If true, include soft-deleted records in results.
    pub include_deleted: Option<bool>,
    /// If true, include inactive records in results.
    pub include_inactive: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_request_offset_calculation() {
        let req = PageRequest { page: 1, per_page: 50 };
        assert_eq!(req.offset(), 0);
        assert_eq!(req.limit(), 50);

        let req2 = PageRequest { page: 3, per_page: 25 };
        assert_eq!(req2.offset(), 50);
        assert_eq!(req2.limit(), 25);
    }

    #[test]
    fn page_request_caps_per_page_at_max() {
        let req = PageRequest { page: 1, per_page: 500 };
        assert_eq!(req.limit(), PageRequest::MAX_PER_PAGE);
        assert_eq!(req.offset(), 0);
    }

    #[test]
    fn page_new_calculates_total_pages_correctly() {
        let req = PageRequest { page: 1, per_page: 25 };
        let page: Page<i32> = Page::new(vec![], 100, &req);
        assert_eq!(page.total_pages, 4);

        let page2: Page<i32> = Page::new(vec![], 101, &req);
        assert_eq!(page2.total_pages, 5);

        let page3: Page<i32> = Page::new(vec![], 0, &req);
        assert_eq!(page3.total_pages, 0);
    }

    #[test]
    fn page_request_default_is_page_1_per_50() {
        let req = PageRequest::default();
        assert_eq!(req.page, 1);
        assert_eq!(req.per_page, 50);
    }
}
