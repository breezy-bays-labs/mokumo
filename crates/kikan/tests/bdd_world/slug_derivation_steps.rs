use cucumber::{then, when};
use kikan::slug::{MAX_SLUG_LEN, Slug, SlugError, derive_slug};

use super::KikanWorld;

pub struct SlugDerivationCtx {
    pub result: Result<Slug, SlugError>,
}

#[when(expr = "I derive a slug from {string}")]
async fn derive_from_input(w: &mut KikanWorld, input: String) {
    w.slug_derivation = Some(SlugDerivationCtx {
        result: derive_slug(&input),
    });
}

#[when("I derive a slug from a 61-character ASCII display name")]
async fn derive_from_overlength(w: &mut KikanWorld) {
    let input = "a".repeat(MAX_SLUG_LEN + 1);
    w.slug_derivation = Some(SlugDerivationCtx {
        result: derive_slug(&input),
    });
}

#[then(expr = "the derived slug is {string}")]
async fn assert_derived_slug(w: &mut KikanWorld, expected: String) {
    let ctx = w.slug_derivation.as_ref().expect("derive_slug was invoked");
    let slug = ctx
        .result
        .as_ref()
        .expect("derive_slug returned an error, expected Ok");
    assert_eq!(slug.as_str(), expected);
}

#[then("derive_slug rejects the input as Unparseable")]
async fn assert_unparseable(w: &mut KikanWorld) {
    let ctx = w.slug_derivation.as_ref().expect("derive_slug was invoked");
    let err = ctx
        .result
        .as_ref()
        .expect_err("expected derive_slug to reject the input");
    assert!(
        matches!(err, SlugError::Unparseable { .. }),
        "expected Unparseable, got {err:?}"
    );
}

#[then(expr = "derive_slug rejects the input as Reserved {string}")]
async fn assert_reserved(w: &mut KikanWorld, expected_name: String) {
    let ctx = w.slug_derivation.as_ref().expect("derive_slug was invoked");
    let err = ctx
        .result
        .as_ref()
        .expect_err("expected derive_slug to reject the input");
    let SlugError::Reserved(name) = err else {
        panic!("expected Reserved, got {err:?}");
    };
    assert_eq!(name, &expected_name);
}

#[then("derive_slug rejects the input as TooLong")]
async fn assert_too_long(w: &mut KikanWorld) {
    let ctx = w.slug_derivation.as_ref().expect("derive_slug was invoked");
    let err = ctx
        .result
        .as_ref()
        .expect_err("expected derive_slug to reject the input");
    assert!(
        matches!(err, SlugError::TooLong { .. }),
        "expected TooLong, got {err:?}"
    );
}
