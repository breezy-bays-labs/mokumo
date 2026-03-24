use cucumber::{World, given, then, when};

#[derive(Debug, Default, World)]
pub struct CoreWorld {
    prefix: String,
    padding: u32,
    result: String,
}

#[given(expr = "a prefix {string} and padding {int}")]
async fn given_prefix_and_padding(w: &mut CoreWorld, prefix: String, padding: i32) {
    w.prefix = prefix;
    w.padding = padding as u32;
}

#[when(expr = "value {int} is formatted")]
async fn when_value_formatted(w: &mut CoreWorld, value: i64) {
    w.result = mokumo_core::sequence::format_sequence_number(&w.prefix, value, w.padding);
}

#[then(expr = "the display number is {string}")]
async fn then_display_number_is(w: &mut CoreWorld, expected: String) {
    assert_eq!(
        w.result, expected,
        "Expected '{}', got '{}'",
        expected, w.result
    );
}
