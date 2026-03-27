use cucumber::given;

#[given(expr = "a value of {bad_type_that_does_not_exist}")]
async fn bad_step(w: &mut World) {}
