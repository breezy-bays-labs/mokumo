use anyhow::Result;

fn main() -> Result<()> {
    let workspace_root = docs_gen::workspace::find_root()?;
    docs_gen::run(&workspace_root, &docs_gen::registry::all())
}
