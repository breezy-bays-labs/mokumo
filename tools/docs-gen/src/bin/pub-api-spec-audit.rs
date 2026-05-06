//! `pub-api-spec-audit` — CC=1 shim. Logic lives in
//! [`docs_gen::pub_api_audit::cli::execute`] (mokumo#654).

fn main() {
    std::process::exit(docs_gen::pub_api_audit::execute(
        std::env::args().skip(1).collect(),
    ));
}
