//! Walk Rust source files to enumerate every `pub` item declaration.
//!
//! "Item" = `fn`, `struct`, `enum`, `trait`, `const`, `static`, `type`,
//! `mod`, plus methods inside `impl Type` blocks. Only **bare `pub`**
//! visibility counts — `pub(crate)`, `pub(super)`, `pub(in path)` are
//! crate-internal and out of scope for the BDD-coverage gate.
//!
//! Path resolution:
//! - File path → module path: `<crate>/src/lib.rs` and `<crate>/src/main.rs`
//!   resolve to the crate root; `<crate>/src/foo.rs` and
//!   `<crate>/src/foo/mod.rs` both resolve to `crate::foo`; nested files
//!   under `<crate>/src/foo/bar.rs` resolve to `crate::foo::bar`.
//! - `mod foo;` declarations are NOT followed transitively — the file-path
//!   layout is the source of truth, matching mokumo's flat module layout.
//! - Item path = `<module-path>::<item-ident>`, with one extra hop for
//!   methods (`<module-path>::<TypeIdent>::<method-ident>`).
//!
//! Span: every emitted item carries `[line_start, line_end]` (1-based,
//! inclusive). Spans are derived from `syn`'s `proc-macro2`-backed
//! source locations (we already depend on `proc-macro2` with
//! `span-locations`).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::{
    ImplItem, Item, ItemConst, ItemEnum, ItemFn, ItemImpl, ItemMod, ItemStatic, ItemStruct,
    ItemTrait, ItemType, Visibility,
};
use walkdir::WalkDir;

/// Item kind tag echoed into the artifact. Stringly-typed because it
/// reaches the user-facing markdown report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PubItemKind {
    Fn,
    Struct,
    Enum,
    Trait,
    Const,
    Static,
    Type,
    Mod,
    Method,
}

impl PubItemKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PubItemKind::Fn => "fn",
            PubItemKind::Struct => "struct",
            PubItemKind::Enum => "enum",
            PubItemKind::Trait => "trait",
            PubItemKind::Const => "const",
            PubItemKind::Static => "static",
            PubItemKind::Type => "type",
            PubItemKind::Mod => "mod",
            PubItemKind::Method => "method",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubItem {
    /// Crate name as Rust ident (e.g. `mokumo_shop`).
    pub crate_name: String,
    /// Fully-qualified item path, e.g. `mokumo_shop::customer::Customer`
    /// or `mokumo_shop::customer::CustomerService::list`.
    pub item_path: String,
    pub kind: PubItemKind,
    /// Repo-relative source file path.
    pub source_file: PathBuf,
    /// 1-based inclusive start line.
    pub source_line_start: u32,
    /// 1-based inclusive end line.
    pub source_line_end: u32,
}

#[derive(Debug, Clone)]
pub struct ParseFinding {
    pub source_file: PathBuf,
    pub reason: String,
}

pub struct WalkOutcome {
    pub items: Vec<PubItem>,
    pub parse_errors: Vec<ParseFinding>,
}

/// Walk every Rust source file under `<crate_dir>/src/` for each crate
/// in `crate_dirs`. Returns the union of items + per-file parse errors.
pub fn walk(crate_dirs: &[(String, PathBuf)]) -> Result<WalkOutcome> {
    let mut items = Vec::new();
    let mut parse_errors = Vec::new();
    for (pkg_name, crate_dir) in crate_dirs {
        walk_one_crate(pkg_name, crate_dir, &mut items, &mut parse_errors);
    }
    items.sort_by(|a, b| a.item_path.cmp(&b.item_path));
    Ok(WalkOutcome {
        items,
        parse_errors,
    })
}

fn walk_one_crate(
    pkg_name: &str,
    crate_dir: &Path,
    items: &mut Vec<PubItem>,
    parse_errors: &mut Vec<ParseFinding>,
) {
    let src_dir = crate_dir.join("src");
    if !src_dir.is_dir() {
        return;
    }
    let crate_ident = crate_name_to_ident(pkg_name);
    for entry in WalkDir::new(&src_dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() || path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let module_path = file_to_module_path(&crate_ident, &src_dir, path);
        match parse_file_into(path, &module_path, items) {
            Ok(()) => {}
            Err(err) => parse_errors.push(ParseFinding {
                source_file: path.to_path_buf(),
                reason: err.to_string(),
            }),
        }
    }
}

fn parse_file_into(path: &Path, module_path: &str, items: &mut Vec<PubItem>) -> Result<()> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let parsed: syn::File =
        syn::parse_file(&raw).with_context(|| format!("parsing {}", path.display()))?;
    for it in &parsed.items {
        collect_item(it, module_path, path, items);
    }
    Ok(())
}

fn collect_item(item: &Item, module_path: &str, path: &Path, out: &mut Vec<PubItem>) {
    match item {
        Item::Fn(f) => collect_fn(f, module_path, path, out),
        Item::Struct(s) => collect_struct(s, module_path, path, out),
        Item::Enum(e) => collect_enum(e, module_path, path, out),
        Item::Trait(t) => collect_trait(t, module_path, path, out),
        Item::Const(c) => collect_const(c, module_path, path, out),
        Item::Static(s) => collect_static(s, module_path, path, out),
        Item::Type(t) => collect_type(t, module_path, path, out),
        Item::Mod(m) => collect_inline_mod(m, module_path, path, out),
        Item::Impl(i) => collect_impl_methods(i, module_path, path, out),
        // Other Item variants (Use, ExternCrate, Macro, …) don't
        // contribute pub-item surface for this gate.
        _ => {}
    }
}

fn collect_fn(item: &ItemFn, module_path: &str, path: &Path, out: &mut Vec<PubItem>) {
    if !is_bare_pub(&item.vis) {
        return;
    }
    push(
        out,
        module_path,
        &item.sig.ident.to_string(),
        PubItemKind::Fn,
        path,
        item.span(),
    );
}

fn collect_struct(item: &ItemStruct, module_path: &str, path: &Path, out: &mut Vec<PubItem>) {
    if is_bare_pub(&item.vis) {
        push(
            out,
            module_path,
            &item.ident.to_string(),
            PubItemKind::Struct,
            path,
            item.span(),
        );
    }
}

fn collect_enum(item: &ItemEnum, module_path: &str, path: &Path, out: &mut Vec<PubItem>) {
    if is_bare_pub(&item.vis) {
        push(
            out,
            module_path,
            &item.ident.to_string(),
            PubItemKind::Enum,
            path,
            item.span(),
        );
    }
}

fn collect_trait(item: &ItemTrait, module_path: &str, path: &Path, out: &mut Vec<PubItem>) {
    if is_bare_pub(&item.vis) {
        push(
            out,
            module_path,
            &item.ident.to_string(),
            PubItemKind::Trait,
            path,
            item.span(),
        );
    }
}

fn collect_const(item: &ItemConst, module_path: &str, path: &Path, out: &mut Vec<PubItem>) {
    if is_bare_pub(&item.vis) {
        push(
            out,
            module_path,
            &item.ident.to_string(),
            PubItemKind::Const,
            path,
            item.span(),
        );
    }
}

fn collect_static(item: &ItemStatic, module_path: &str, path: &Path, out: &mut Vec<PubItem>) {
    if is_bare_pub(&item.vis) {
        push(
            out,
            module_path,
            &item.ident.to_string(),
            PubItemKind::Static,
            path,
            item.span(),
        );
    }
}

fn collect_type(item: &ItemType, module_path: &str, path: &Path, out: &mut Vec<PubItem>) {
    if is_bare_pub(&item.vis) {
        push(
            out,
            module_path,
            &item.ident.to_string(),
            PubItemKind::Type,
            path,
            item.span(),
        );
    }
}

fn collect_inline_mod(item: &ItemMod, module_path: &str, path: &Path, out: &mut Vec<PubItem>) {
    let inline_path = format!("{module_path}::{}", item.ident);
    if is_bare_pub(&item.vis) {
        push(
            out,
            module_path,
            &item.ident.to_string(),
            PubItemKind::Mod,
            path,
            item.span(),
        );
    }
    if let Some((_, items)) = item.content.as_ref() {
        for nested in items {
            collect_item(nested, &inline_path, path, out);
        }
    }
}

fn collect_impl_methods(item: &ItemImpl, module_path: &str, path: &Path, out: &mut Vec<PubItem>) {
    // Trait impls export under the trait/type — we anchor the methods
    // under the SELF type so the gate sees `Type::method`, not
    // `Trait::method`. For inherent impls, same anchor.
    let Some(type_ident) = self_type_ident(&item.self_ty) else {
        return;
    };
    for impl_item in &item.items {
        if let ImplItem::Fn(method) = impl_item {
            if !is_bare_pub(&method.vis) {
                continue;
            }
            let path_seg = format!("{type_ident}::{}", method.sig.ident);
            push(
                out,
                module_path,
                &path_seg,
                PubItemKind::Method,
                path,
                method.span(),
            );
        }
    }
}

fn push(
    out: &mut Vec<PubItem>,
    module_path: &str,
    leaf: &str,
    kind: PubItemKind,
    file: &Path,
    span: proc_macro2::Span,
) {
    let crate_name = module_path
        .split("::")
        .next()
        .unwrap_or(module_path)
        .to_string();
    let item_path = format!("{module_path}::{leaf}");
    let start = u32::try_from(span.start().line).unwrap_or(0);
    let end = u32::try_from(span.end().line).unwrap_or(start);
    out.push(PubItem {
        crate_name,
        item_path,
        kind,
        source_file: file.to_path_buf(),
        source_line_start: start,
        source_line_end: end.max(start),
    });
}

fn is_bare_pub(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// Pluck the leaf identifier out of an `impl <Type>` self-type. We don't
/// resolve generic args; `impl<T> Foo<T>` reads as `Foo`.
fn self_type_ident(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(tp) = ty else {
        return None;
    };
    Some(tp.path.segments.last()?.ident.to_string())
}

/// Map a source file path under `<crate>/src/` to a Rust module path.
/// `<crate>/src/lib.rs` and `<crate>/src/main.rs` map to the crate root;
/// `<crate>/src/foo.rs` and `<crate>/src/foo/mod.rs` both map to
/// `<crate>::foo`; nested files map by joining intermediate dir names.
pub fn file_to_module_path(crate_ident: &str, src_dir: &Path, file: &Path) -> String {
    let Ok(rel) = file.strip_prefix(src_dir) else {
        return crate_ident.to_string();
    };
    let mut segs: Vec<String> = Vec::new();
    let comps: Vec<_> = rel.components().collect();
    for (i, comp) in comps.iter().enumerate() {
        let s = comp.as_os_str().to_string_lossy();
        if i == comps.len() - 1 {
            // Leaf: `lib.rs` / `main.rs` / `mod.rs` contribute nothing;
            // `foo.rs` contributes `foo`.
            let stem = s.trim_end_matches(".rs");
            if matches!(stem, "lib" | "main" | "mod") {
                continue;
            }
            segs.push(stem.to_string());
        } else {
            segs.push(s.to_string());
        }
    }
    if segs.is_empty() {
        crate_ident.to_string()
    } else {
        format!("{crate_ident}::{}", segs.join("::"))
    }
}

/// Cargo package names use `-`; Rust idents use `_`. Match the conversion
/// rustc does for the implicit `extern crate` name.
fn crate_name_to_ident(pkg_name: &str) -> String {
    pkg_name.replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_crate(root: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        for (rel, body) in files {
            let abs = dir.join("src").join(rel);
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(abs, body).unwrap();
        }
        dir
    }

    fn names(items: &[PubItem]) -> Vec<&str> {
        items.iter().map(|i| i.item_path.as_str()).collect()
    }

    #[test]
    fn walks_pub_fn_struct_enum_in_lib_rs() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                r"pub fn run() {}
pub struct Config { pub field: i32 }
pub enum Status { Ok, Err }
fn private() {}
",
            )],
        );
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        let names = names(&outcome.items);
        assert!(names.contains(&"demo::run"), "{names:?}");
        assert!(names.contains(&"demo::Config"), "{names:?}");
        assert!(names.contains(&"demo::Status"), "{names:?}");
        assert!(!names.iter().any(|n| n.ends_with("private")));
    }

    #[test]
    fn skips_pub_crate_pub_super() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                r"pub(crate) fn crate_only() {}
pub(super) fn super_only() {}
pub fn really_public() {}
",
            )],
        );
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        let names = names(&outcome.items);
        assert_eq!(names, vec!["demo::really_public"]);
    }

    #[test]
    fn walks_methods_in_impl_blocks() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                r"pub struct Customer;
impl Customer {
    pub fn new() -> Self { Customer }
    pub fn list(&self) {}
    fn private(&self) {}
}
",
            )],
        );
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        let names = names(&outcome.items);
        assert!(names.contains(&"demo::Customer::new"));
        assert!(names.contains(&"demo::Customer::list"));
        assert!(!names.iter().any(|n| n.ends_with("::private")));
    }

    #[test]
    fn walks_nested_files_into_module_paths() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[
                ("lib.rs", "pub mod customer;\n"),
                ("customer/mod.rs", "pub mod domain;\n"),
                ("customer/domain.rs", "pub fn foo() {}\n"),
            ],
        );
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        let names = names(&outcome.items);
        assert!(names.contains(&"demo::customer::domain::foo"), "{names:?}");
    }

    #[test]
    fn walks_inline_pub_mod_into_path() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                r"pub mod inner {
    pub fn nested() {}
    pub struct Item;
}
",
            )],
        );
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        let names = names(&outcome.items);
        assert!(names.contains(&"demo::inner::nested"), "{names:?}");
        assert!(names.contains(&"demo::inner::Item"), "{names:?}");
        assert!(names.contains(&"demo::inner"), "{names:?}");
    }

    #[test]
    fn walks_pub_const_static_type() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                "pub const MAX: u32 = 10;\npub static NAME: &str = \"x\";\npub type Id = u64;\n",
            )],
        );
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        let names = names(&outcome.items);
        assert!(names.contains(&"demo::MAX"));
        assert!(names.contains(&"demo::NAME"));
        assert!(names.contains(&"demo::Id"));
    }

    #[test]
    fn walks_pub_trait() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[("lib.rs", "pub trait Repo { fn list(&self); }\n")],
        );
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        let names = names(&outcome.items);
        assert!(names.contains(&"demo::Repo"));
    }

    #[test]
    fn span_lines_are_one_based_inclusive() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[("lib.rs", "\n\npub fn x() {\n    let y = 1;\n}\n")],
        );
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        let item = outcome
            .items
            .iter()
            .find(|i| i.item_path == "demo::x")
            .unwrap();
        assert_eq!(item.source_line_start, 3);
        assert!(item.source_line_end >= 5);
    }

    #[test]
    fn parse_error_recorded_when_file_is_unparseable() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[
                ("lib.rs", "pub fn good() {}\n"),
                ("garbage.rs", "this is not :: rust @ all\n"),
            ],
        );
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        assert!(!outcome.items.is_empty());
        assert_eq!(outcome.parse_errors.len(), 1);
        assert!(outcome.parse_errors[0].source_file.ends_with("garbage.rs"));
    }

    #[test]
    fn missing_src_dir_is_silently_skipped() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("no-src");
        fs::create_dir_all(&dir).unwrap();
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        assert!(outcome.items.is_empty());
        assert!(outcome.parse_errors.is_empty());
    }

    #[test]
    fn crate_name_dashes_become_underscores() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "kikan-events",
            &[("lib.rs", "pub fn emit() {}\n")],
        );
        let outcome = walk(&[("kikan-events".into(), dir)]).unwrap();
        let names = names(&outcome.items);
        assert!(names.contains(&"kikan_events::emit"), "{names:?}");
    }

    #[test]
    fn methods_anchor_on_self_type_for_trait_impls() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                r"pub trait Repo { fn list(&self) -> Vec<i32>; }
pub struct Memory;
impl Repo for Memory {
    fn list(&self) -> Vec<i32> { Vec::new() }
}
",
            )],
        );
        let outcome = walk(&[("demo".into(), dir)]).unwrap();
        let names = names(&outcome.items);
        // Trait method impls keep their visibility from the trait — the
        // walker emits them under the SELF type. The default trait
        // method visibility is the trait's, so unless re-declared `pub`
        // the impl method isn't bare-pub. This test pins that behavior.
        assert!(names.contains(&"demo::Memory"));
        assert!(names.contains(&"demo::Repo"));
        assert!(
            !names.contains(&"demo::Memory::list"),
            "default trait-impl method visibility is not bare-pub"
        );
    }

    #[test]
    fn file_to_module_path_known_layouts() {
        let src = Path::new("/x/crate/src");
        assert_eq!(
            file_to_module_path("demo", src, &src.join("lib.rs")),
            "demo"
        );
        assert_eq!(
            file_to_module_path("demo", src, &src.join("main.rs")),
            "demo"
        );
        assert_eq!(
            file_to_module_path("demo", src, &src.join("foo.rs")),
            "demo::foo"
        );
        assert_eq!(
            file_to_module_path("demo", src, &src.join("foo/mod.rs")),
            "demo::foo"
        );
        assert_eq!(
            file_to_module_path("demo", src, &src.join("foo/bar.rs")),
            "demo::foo::bar"
        );
    }
}
