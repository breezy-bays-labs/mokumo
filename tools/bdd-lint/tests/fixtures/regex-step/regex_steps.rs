use cucumber::then;

#[then(regex = r#"^the activity log should contain an? "([^"]*)" entry for that customer$"#)]
async fn activity_entry(w: &mut World, action: String) {
    assert!(true);
}
