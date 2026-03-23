use mokumo_core::pagination::PageParams;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

impl PaginationParams {
    pub fn into_page_params(self) -> PageParams {
        PageParams::new(self.page, self.per_page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_values_produce_defaults() {
        let params = PaginationParams {
            page: None,
            per_page: None,
        }
        .into_page_params();
        assert_eq!(params.page(), 1);
        assert_eq!(params.per_page(), 25);
    }

    #[test]
    fn some_values_pass_through() {
        let params = PaginationParams {
            page: Some(3),
            per_page: Some(50),
        }
        .into_page_params();
        assert_eq!(params.page(), 3);
        assert_eq!(params.per_page(), 50);
    }
}
