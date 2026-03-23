pub const DEFAULT_PER_PAGE: u32 = 25;
pub const MAX_PER_PAGE: u32 = 100;

pub struct PageParams {
    page: u32,
    per_page: u32,
}

impl PageParams {
    pub fn new(page: Option<u32>, per_page: Option<u32>) -> Self {
        let page = match page {
            Some(p) if p >= 1 => p,
            _ => 1,
        };
        let per_page = match per_page {
            Some(pp) if pp >= 1 && pp <= MAX_PER_PAGE => pp,
            Some(pp) if pp > MAX_PER_PAGE => MAX_PER_PAGE,
            _ => DEFAULT_PER_PAGE,
        };
        Self { page, per_page }
    }

    pub fn page(&self) -> u32 {
        self.page
    }

    pub fn per_page(&self) -> u32 {
        self.per_page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_page_is_1() {
        assert_eq!(PageParams::new(None, None).page(), 1);
    }

    #[test]
    fn default_per_page_is_25() {
        assert_eq!(PageParams::new(None, None).per_page(), 25);
    }

    #[test]
    fn page_zero_clamped_to_1() {
        assert_eq!(PageParams::new(Some(0), None).page(), 1);
    }

    #[test]
    fn per_page_zero_clamped_to_default() {
        assert_eq!(PageParams::new(None, Some(0)).per_page(), 25);
    }

    #[test]
    fn per_page_above_max_clamped_to_100() {
        assert_eq!(PageParams::new(None, Some(200)).per_page(), 100);
    }

    #[test]
    fn valid_values_passed_through() {
        let params = PageParams::new(Some(3), Some(50));
        assert_eq!(params.page(), 3);
        assert_eq!(params.per_page(), 50);
    }
}
