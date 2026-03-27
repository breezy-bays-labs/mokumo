use super::InventoryWorld;
use cucumber::{given, then, when};

#[given("an empty warehouse")]
async fn empty_warehouse(w: &mut InventoryWorld) {
    w.items.clear();
}

#[when(expr = "an item {string} is added with quantity {int}")]
async fn add_item(w: &mut InventoryWorld, name: String, qty: i32) {
    w.items.insert(name, qty);
}

#[then(expr = "the inventory should contain {string}")]
async fn inventory_contains(w: &mut InventoryWorld, name: String) {
    assert!(w.items.contains_key(&name));
}
