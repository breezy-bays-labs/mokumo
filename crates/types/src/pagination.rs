use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PaginatedList<T: TS> {
    pub items: Vec<T>,
    #[ts(type = "number")]
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
}

impl<T: TS> PaginatedList<T> {
    pub fn new(items: Vec<T>, total: i64, page: u32, per_page: u32) -> Self {
        let total_pages = if total <= 0 || per_page == 0 {
            0
        } else {
            let pages = (total as u64).div_ceil(per_page as u64);
            pages.min(u32::MAX as u64) as u32
        };
        Self {
            items,
            total,
            page,
            per_page,
            total_pages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HealthResponse;

    #[test]
    fn export_bindings() {
        PaginatedList::<HealthResponse>::export_all()
            .expect("Failed to export TypeScript bindings");
    }

    #[test]
    fn total_pages_exact_division() {
        let items: Vec<HealthResponse> = vec![];
        let list = PaginatedList::new(items, 100, 1, 25);
        assert_eq!(list.total_pages, 4);
    }

    #[test]
    fn total_pages_with_remainder() {
        let items: Vec<HealthResponse> = vec![];
        let list = PaginatedList::new(items, 101, 1, 25);
        assert_eq!(list.total_pages, 5);
    }

    #[test]
    fn total_pages_zero_total() {
        let list = PaginatedList::<HealthResponse>::new(vec![], 0, 1, 25);
        assert_eq!(list.total_pages, 0);
    }

    #[test]
    fn total_pages_single_item() {
        let items = vec![HealthResponse {
            status: "ok".into(),
            version: "0.1.0".into(),
            uptime_seconds: 0,
            database: "ok".into(),
        }];
        let list = PaginatedList::new(items, 1, 1, 25);
        assert_eq!(list.total_pages, 1);
    }

    #[test]
    fn total_pages_per_page_zero_returns_zero() {
        let list = PaginatedList::<HealthResponse>::new(vec![], 10, 1, 0);
        assert_eq!(list.total_pages, 0);
    }

    #[test]
    fn total_pages_large_total_saturates() {
        let list = PaginatedList::<HealthResponse>::new(vec![], 5_000_000_000, 1, 100);
        assert_eq!(list.total_pages, 50_000_000);
    }

    #[test]
    fn serde_roundtrip() {
        let items = vec![HealthResponse {
            status: "ok".into(),
            version: "0.1.0".into(),
            uptime_seconds: 0,
            database: "ok".into(),
        }];
        let list = PaginatedList::new(items, 1, 1, 25);
        let json = serde_json::to_string(&list).unwrap();
        let restored: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(restored["total_pages"], serde_json::json!(1));
        assert_eq!(restored["page"], serde_json::json!(1));
        assert_eq!(restored["per_page"], serde_json::json!(25));
    }
}
