use vergen_gitcl::{BuildBuilder, CargoBuilder, Emitter, GitclBuilder, RustcBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let build = BuildBuilder::default().build_timestamp(true).build()?;
    let cargo = CargoBuilder::default().target_triple(true).build()?;
    let rustc = RustcBuilder::default().semver(true).build()?;

    let mut emitter = Emitter::default();
    emitter
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&rustc)?;

    // Git metadata is best-effort: source archives lack .git
    if let Ok(gitcl) = GitclBuilder::default()
        .sha(true)
        .commit_timestamp(true)
        .build()
    {
        emitter.add_instructions(&gitcl)?;
    } else {
        println!("cargo:rustc-env=VERGEN_GIT_SHA=unknown");
    }

    emitter.emit()?;
    Ok(())
}
