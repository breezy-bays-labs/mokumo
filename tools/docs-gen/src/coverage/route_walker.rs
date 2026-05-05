//! Walk Rust source files to extract `(method, url_path, handler_rust_path)`
//! triples from Axum router builders.
//!
//! The walker handles three classes of route declaration:
//!
//! 1. **Direct**: `.route("/api/users", post(handler))` inside a function
//!    that builds a `Router`. Multiple methods per route — e.g.
//!    `get(h1).post(h2)` — emit one entry per method.
//!
//! 2. **Nested**: `.nest("/api/foo", crate::foo::router())` mounts another
//!    builder fn under a prefix. The walker tracks the call-graph and
//!    resolves the full URL path (`/api/foo` + the nested builder's own
//!    `.route("/")` literal).
//!
//! 3. **Inline anonymous**: `.nest("/x", Router::new().route("/", h))`
//!    — the inner `.route()` is visited in-place; its prefix accumulates
//!    from the surrounding `.nest()` chain.
//!
//! Handler-symbol resolution:
//! - Fully-qualified path (`crate::user::handler::create`) — expanded by
//!   replacing `crate` with the file's crate name.
//! - Two-or-more-segment path (`recover::handler`) — first segment looked
//!   up against `use` items in the same file.
//! - Bare ident (`create_user`) — looked up against `use` items; falls
//!   back to "module-local" if no `use` matches.
//!
//! What the walker explicitly does NOT do:
//! - Cross-file `mod foo;` resolution (the heuristic file-path-to-module
//!   mapping in [`file_module_path`] is sufficient because Mokumo's
//!   handler files all live under `crates/<crate>/src/` with predictable
//!   layout — escape hatch is to qualify handler refs with `crate::`).
//! - Macro-generated routes (`routes!()` etc. don't appear in mokumo).
//! - `axum::routing::on(MethodFilter::GET, h)` — not used in mokumo.
//!
//! Each unresolvable route is recorded as a [`UnresolvableRouteFinding`]
//! with the source file + line + reason. The producer fails the build
//! when any are present.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use syn::{Expr, ExprMethodCall, ExprPath, ItemUse, Lit, UseTree};
use walkdir::WalkDir;

use crate::coverage::crap_exclusions;

/// One `(method, url_path, handler_rust_path)` triple resolved from source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteEntry {
    /// HTTP method in uppercase (`GET`, `POST`, …).
    pub method: String,
    /// Full URL path, including any prefix accumulated through `.nest()`.
    pub path: String,
    /// Fully-qualified Rust path of the handler function
    /// (e.g. `mokumo_shop::user::handler::create_user`).
    pub rust_path: String,
    /// Crate-name-as-Rust-ident (e.g. `mokumo_shop`).
    pub crate_name: String,
    /// Source file the `.route()` call lives in.
    pub source_file: PathBuf,
    /// 1-based line number of the `.route()` call.
    pub source_line: u32,
}

/// A route the walker found but could not resolve to a Rust path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvableRouteFinding {
    pub route_literal: String,
    pub source_file: PathBuf,
    pub source_line: u32,
    pub reason: String,
}

/// Result of [`walk`]: resolved routes plus any unresolvable entries
/// (= source-level findings the producer surfaces in diagnostics).
pub struct WalkOutcome {
    pub routes: Vec<RouteEntry>,
    pub unresolvable: Vec<UnresolvableRouteFinding>,
}

/// Walk every `.rs` file under each `crate_dir` and extract routes.
///
/// `crate_dirs` is a list of `(crate_name_ident, crate_dir)` pairs — the
/// crate-name-as-Rust-ident (`mokumo_shop`) and the directory holding its
/// `Cargo.toml`. The walker scans each crate's `src/` recursively.
pub fn walk(crate_dirs: &[(String, PathBuf)]) -> Result<WalkOutcome> {
    let parsed = parse_crate_sources(crate_dirs)?;
    let prefixes = resolve_prefixes(&parsed.builders, &parsed.nest_edges);
    let mut routes = emit_routes(&parsed.builders, &prefixes);
    routes.sort_by(|a, b| {
        a.crate_name
            .cmp(&b.crate_name)
            .then(a.method.cmp(&b.method))
            .then(a.path.cmp(&b.path))
    });
    Ok(WalkOutcome {
        routes,
        unresolvable: parsed.unresolvable,
    })
}

/// Pass 1 result: all builder functions, nest edges, and per-file
/// unresolvable findings collected by walking every `.rs` source under
/// each crate's `src/`.
struct ParsedSources {
    builders: HashMap<String, BuilderFn>,
    nest_edges: Vec<NestEdge>,
    unresolvable: Vec<UnresolvableRouteFinding>,
}

/// Pass 1 — visit every source file across `crate_dirs` and collect the
/// builders / nest edges / unresolvable findings into one bucket.
fn parse_crate_sources(crate_dirs: &[(String, PathBuf)]) -> Result<ParsedSources> {
    let mut acc = ParsedSources {
        builders: HashMap::new(),
        nest_edges: Vec::new(),
        unresolvable: Vec::new(),
    };
    for (crate_name, crate_dir) in crate_dirs {
        let src_dir = crate_dir.join("src");
        if !src_dir.is_dir() {
            continue;
        }
        scan_src_tree(crate_name, &src_dir, &mut acc)?;
    }
    Ok(acc)
}

/// Walk one crate's `src/` tree, parse each `.rs` file, and merge the
/// per-file visitor output into `acc`.
fn scan_src_tree(crate_name: &str, src_dir: &Path, acc: &mut ParsedSources) -> Result<()> {
    for entry in WalkDir::new(src_dir).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().is_none_or(|e| e != "rs") {
            continue;
        }
        visit_one_file(crate_name, src_dir, entry.path(), acc)?;
    }
    Ok(())
}

/// Parse one `.rs` file and merge its visitor output into `acc`. Skips
/// files that don't parse (e.g. `build.rs` with non-rustc syntax).
fn visit_one_file(
    crate_name: &str,
    src_dir: &Path,
    file_path: &Path,
    acc: &mut ParsedSources,
) -> Result<()> {
    let source = std::fs::read_to_string(file_path)
        .with_context(|| format!("reading {}", file_path.display()))?;
    let Ok(parsed) = syn::parse_file(&source) else {
        return Ok(());
    };
    let crate_ident = crap_exclusions::to_ident(crate_name);
    let module_path = file_module_path(&crate_ident, src_dir, file_path);
    let mut visitor = FileVisitor::new(crate_name, &crate_ident, &module_path, file_path);
    visitor.visit_file(&parsed);
    for (path, b) in visitor.builders {
        acc.builders.insert(path, b);
    }
    acc.nest_edges.extend(visitor.nest_edges);
    acc.unresolvable.extend(visitor.unresolvable);
    Ok(())
}

/// Pass 3 — fan each builder's `(prefix, route, method)` triple out into
/// flat [`RouteEntry`] rows. Caller sorts.
fn emit_routes(
    builders: &HashMap<String, BuilderFn>,
    prefixes: &HashMap<String, String>,
) -> Vec<RouteEntry> {
    let mut routes = Vec::new();
    for (builder_path, builder) in builders {
        let prefix = prefixes.get(builder_path).cloned().unwrap_or_default();
        for r in &builder.routes {
            let full_path = join_url_path(&prefix, &r.literal_path);
            for method in &r.methods {
                routes.push(RouteEntry {
                    method: method.upper.clone(),
                    path: full_path.clone(),
                    rust_path: method.handler_rust_path.clone(),
                    crate_name: builder.crate_name.clone(),
                    source_file: builder.source_file.clone(),
                    source_line: r.source_line,
                });
            }
        }
    }
    routes
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct BuilderFn {
    crate_name: String,
    source_file: PathBuf,
    routes: Vec<RouteCall>,
}

#[derive(Debug)]
struct RouteCall {
    literal_path: String,
    methods: Vec<RouteMethod>,
    source_line: u32,
}

#[derive(Debug)]
struct RouteMethod {
    /// Uppercase method name (`GET`, `POST`, …).
    upper: String,
    /// Resolved Rust path of the handler.
    handler_rust_path: String,
}

#[derive(Debug)]
struct NestEdge {
    /// Builder fn that contains the `.nest()` call.
    parent: String,
    /// Builder fn referenced as the nested target (e.g. `crate::foo::router`).
    /// `None` for inline-anonymous nests (which are handled by visiting the
    /// inner expression in the same pass — they have no separate builder fn).
    child: Option<String>,
    /// URL prefix supplied to the `.nest()` call.
    prefix: String,
}

// ---------------------------------------------------------------------------
// Per-file visitor
// ---------------------------------------------------------------------------

struct FileVisitor<'a> {
    /// Cargo package name (e.g. `mokumo-shop`) — flows verbatim into
    /// `BuilderFn.crate_name` / `RouteEntry.crate_name` so the artifact's
    /// drill-down keeps hyphens. Path resolution must NOT use this — Rust
    /// idents have underscores. Use [`Self::crate_ident`] for that.
    crate_name: &'a str,
    /// Rust-ident form of the crate name (`mokumo_shop`) — used to
    /// resolve `crate::…` paths and `use` items into fully-qualified
    /// paths matched against demangled symbols.
    crate_ident: &'a str,
    file_module_path: &'a str,
    source_file: &'a Path,
    /// `use` items in this file: ident-or-alias → fully qualified path.
    use_map: HashMap<String, String>,
    /// Sibling module declarations in this file (`mod xxx;` /
    /// `pub mod xxx;`) — the names that, when used as a multi-segment
    /// path head with no matching `use`, refer to a child module of
    /// `file_module_path`. Without this set, `.post(login::login)` from
    /// a parent `mod.rs` resolves to literal `login::login` instead of
    /// `<crate>::<...>::login::login`.
    module_decls: std::collections::HashSet<String>,
    /// Stack of enclosing fn paths (innermost on top). Empty when not
    /// inside any fn (e.g. between items at module level).
    fn_stack: Vec<String>,
    /// Stack of `.nest()` prefixes accumulated by the current method-call
    /// expression chain. Inline-anonymous nests push a frame; resolved
    /// nests don't (they trigger a NestEdge instead).
    inline_prefix_stack: Vec<String>,
    /// Builders discovered in this file.
    builders: HashMap<String, BuilderFn>,
    /// Nest edges discovered in this file.
    nest_edges: Vec<NestEdge>,
    /// Unresolvable findings.
    unresolvable: Vec<UnresolvableRouteFinding>,
}

impl<'a> FileVisitor<'a> {
    fn new(
        crate_name: &'a str,
        crate_ident: &'a str,
        file_module_path: &'a str,
        source_file: &'a Path,
    ) -> Self {
        Self {
            crate_name,
            crate_ident,
            file_module_path,
            source_file,
            use_map: HashMap::new(),
            module_decls: std::collections::HashSet::new(),
            fn_stack: Vec::new(),
            inline_prefix_stack: Vec::new(),
            builders: HashMap::new(),
            nest_edges: Vec::new(),
            unresolvable: Vec::new(),
        }
    }

    // span_line is now a free helper (see [`span_line`] below) so the
    // visitor doesn't carry an unused `&self`.

    /// Resolve `crate::a::b` / `super::a::b` / `self::a::b` / bare-or-qualified
    /// path to a fully-qualified Rust path under the file's crate.
    fn resolve_path(&self, segments: &[String]) -> Option<String> {
        if segments.is_empty() {
            return None;
        }
        let head = segments[0].as_str();
        let tail = &segments[1..];
        let resolved = match head {
            "crate" => join_path(self.crate_ident, tail),
            "self" => join_path(self.file_module_path, tail),
            "super" => join_path(parent_module(self.file_module_path), tail),
            other => self.resolve_other_head(other, segments, tail),
        };
        Some(resolved)
    }

    /// Resolution branches for any non-keyword head — split out so
    /// `resolve_path` is a clean dispatch and the four cases here are
    /// individually CC-cheap.
    fn resolve_other_head(&self, head: &str, segments: &[String], tail: &[String]) -> String {
        if let Some(use_target) = self.use_map.get(head) {
            // Bare ident or two-segment chain via use.
            return join_path(use_target, tail);
        }
        if segments.len() == 1 {
            // Single ident with no `use` match — assume module-local.
            return join_path(self.file_module_path, &[head.to_string()]);
        }
        if self.module_decls.contains(head) {
            // Multi-segment whose head names a sibling module declared in
            // this file (`mod foo;`). Resolve under
            // `file_module_path::foo::…` — without this branch, patterns
            // like `.post(login::login)` from a parent `mod.rs` would
            // mis-resolve to literal `login::login`.
            let mut prefix = self.file_module_path.to_string();
            prefix.push_str("::");
            prefix.push_str(head);
            return join_path(&prefix, tail);
        }
        // Multi-segment with unresolvable head — assume it's a
        // qualified-by-crate-name reference (e.g. `axum::Router`), which
        // the producer is not interested in. Caller decides whether to
        // record this as unresolvable.
        join_path(head, tail)
    }

    fn current_fn(&self) -> Option<&str> {
        self.fn_stack.last().map(String::as_str)
    }

    fn record_unresolvable(&mut self, lit: &str, line: u32, reason: impl Into<String>) {
        self.unresolvable.push(UnresolvableRouteFinding {
            route_literal: lit.to_string(),
            source_file: self.source_file.to_path_buf(),
            source_line: line,
            reason: reason.into(),
        });
    }
}

impl<'ast> Visit<'ast> for FileVisitor<'_> {
    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        collect_use_items(&item.tree, &[], &mut self.use_map, self.crate_ident);
        syn::visit::visit_item_use(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        // Track sibling module declarations so multi-segment paths like
        // `login::login` resolve under `file_module_path` instead of being
        // treated as external-crate qualified. Both bare `mod foo;` and
        // inline `mod foo { ... }` count — once `foo` is a sibling, any
        // `foo::…` reference in the same file roots there.
        self.module_decls.insert(item.ident.to_string());
        syn::visit::visit_item_mod(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let fn_name = item.sig.ident.to_string();
        let mut fn_path = self.file_module_path.to_string();
        fn_path.push_str("::");
        fn_path.push_str(&fn_name);
        self.fn_stack.push(fn_path);
        syn::visit::visit_item_fn(self, item);
        self.fn_stack.pop();
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        // Methods on impls — treat their path as `<file_module>::<method>`
        // for our purposes (we don't need precise impl-block resolution
        // because Axum router builders are free fns by convention).
        let fn_name = item.sig.ident.to_string();
        let mut fn_path = self.file_module_path.to_string();
        fn_path.push_str("::");
        fn_path.push_str(&fn_name);
        self.fn_stack.push(fn_path);
        syn::visit::visit_impl_item_fn(self, item);
        self.fn_stack.pop();
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let method_name = call.method.to_string();
        match method_name.as_str() {
            "route" => {
                self.handle_route_call(call);
                syn::visit::visit_expr_method_call(self, call);
            }
            "nest" => {
                // `handle_nest_call` already walks `target_expr` itself
                // (with the prefix on `inline_prefix_stack`); falling
                // through to the default visitor would walk it a second
                // time without the prefix and emit duplicate routes.
                // Walk only the receiver so chained `.route(...).nest(...)`
                // siblings still get visited.
                self.handle_nest_call(call);
                syn::visit::visit_expr(self, &call.receiver);
            }
            _ => syn::visit::visit_expr_method_call(self, call),
        }
    }
}

impl FileVisitor<'_> {
    fn handle_route_call(&mut self, call: &ExprMethodCall) {
        let line = span_line(&call.method);
        let Some(literal_path) = call.args.first().and_then(extract_string_literal) else {
            return;
        };
        let Some(handler_expr) = call.args.iter().nth(1) else {
            return;
        };
        let methods = match collect_method_router(handler_expr) {
            MethodRouterCollect::Resolved(methods) => methods,
            MethodRouterCollect::Skip => return,
        };
        if methods.is_empty() {
            // Probably an axum middleware-only `.route()` — skip silently.
            return;
        }
        let mut resolved_methods = Vec::with_capacity(methods.len());
        for (method_name, handler_path) in methods {
            let Some(rust_path) = self.resolve_path(&handler_path) else {
                self.record_unresolvable(
                    &literal_path,
                    line,
                    format!("could not resolve handler ident for `{method_name}`"),
                );
                return;
            };
            resolved_methods.push(RouteMethod {
                upper: method_name.to_uppercase(),
                handler_rust_path: rust_path,
            });
        }
        let Some(builder_path) = self.current_fn().map(str::to_string) else {
            self.record_unresolvable(
                &literal_path,
                line,
                "`.route()` call is not inside a fn — cannot identify builder",
            );
            return;
        };
        // Combine inline-prefix stack onto literal path for inline-anonymous nests.
        let combined_path = self
            .inline_prefix_stack
            .iter()
            .fold(String::new(), |acc, p| join_url_path(&acc, p));
        let combined_path = join_url_path(&combined_path, &literal_path);
        self.builders
            .entry(builder_path)
            .or_insert_with(|| BuilderFn {
                crate_name: self.crate_name.to_string(),
                source_file: self.source_file.to_path_buf(),
                routes: Vec::new(),
            })
            .routes
            .push(RouteCall {
                literal_path: combined_path,
                methods: resolved_methods,
                source_line: line,
            });
    }

    fn handle_nest_call(&mut self, call: &ExprMethodCall) {
        let line = span_line(&call.method);
        let Some(prefix) = call.args.first().and_then(extract_string_literal) else {
            return;
        };
        let Some(target_expr) = call.args.iter().nth(1) else {
            return;
        };
        let Some(parent_builder) = self.current_fn().map(str::to_string) else {
            self.record_unresolvable(
                &prefix,
                line,
                "`.nest()` call is not inside a fn — cannot identify builder",
            );
            return;
        };
        // Two cases for the target expr:
        //  1. `crate::foo::router()` — a function call yielding a Router.
        //     Record a NestEdge.
        //  2. Inline `Router::new().route(...)` — push prefix onto the
        //     inline_prefix_stack for the duration of visiting the inner
        //     expression so any `.route()` inside picks up the prefix.
        // First case: `.nest("/x", crate::foo::router())` — the target is
        // a function call whose path resolves to a builder fn we can track
        // through a NestEdge. Anything else (inline `Router::new().route(...)`
        // chain, custom MethodRouter expression, …) falls through to
        // visit-the-inner-expr with the prefix on the stack so its `.route()`
        // calls pick the prefix up directly.
        if let Expr::Call(call_inner) = target_expr
            && let Expr::Path(ExprPath { path, .. }) = &*call_inner.func
        {
            let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            if let Some(child) = self.resolve_path(&segments) {
                self.nest_edges.push(NestEdge {
                    parent: parent_builder,
                    child: Some(child),
                    prefix,
                });
                return;
            }
        }
        self.inline_prefix_stack.push(prefix.clone());
        syn::visit::visit_expr(self, target_expr);
        self.inline_prefix_stack.pop();
    }
}

// ---------------------------------------------------------------------------
// Method-router chain extraction
// ---------------------------------------------------------------------------

/// Result of extracting handlers from the second arg of `.route()`.
enum MethodRouterCollect {
    /// `(method_name, handler_path_segments)` pairs.
    Resolved(Vec<(String, Vec<String>)>),
    /// The expression isn't a method-router chain we recognise — skip.
    Skip,
}

/// Walk a method-router chain like `get(h1).post(h2).put(h3)` collecting
/// each `(method, handler_path)` pair. Returns `Skip` for shapes we don't
/// recognise (middleware-only routes, custom MethodRouter constructions).
fn collect_method_router(expr: &Expr) -> MethodRouterCollect {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut current = expr;
    loop {
        match current {
            Expr::Call(call) => {
                let Some(pair) = head_call_method_handler(call) else {
                    return MethodRouterCollect::Skip;
                };
                out.push(pair);
                return MethodRouterCollect::Resolved(out);
            }
            Expr::MethodCall(mcall) => {
                let Some(pair) = chain_call_method_handler(mcall) else {
                    return MethodRouterCollect::Skip;
                };
                out.push(pair);
                current = &mcall.receiver;
            }
            _ => return MethodRouterCollect::Skip,
        }
    }
}

/// Pull `(method, handler_path)` from the **head** of a method-router
/// chain — `get(handler)` — returning `None` when the call isn't
/// recognisable as `<method-router-ctor>(handler)`.
fn head_call_method_handler(call: &syn::ExprCall) -> Option<(String, Vec<String>)> {
    let Expr::Path(ExprPath { path, .. }) = &*call.func else {
        return None;
    };
    let last = path.segments.last().map(|s| s.ident.to_string())?;
    if !is_method_router_constructor(&last) {
        return None;
    }
    let handler_arg = call.args.first()?;
    let handler_path = expr_to_path_segments(handler_arg)?;
    Some((last, handler_path))
}

/// Pull `(method, handler_path)` from a **chained** method-router call —
/// `.post(handler)` on top of a receiver. Returns `None` when the call
/// isn't a method-router method.
fn chain_call_method_handler(mcall: &ExprMethodCall) -> Option<(String, Vec<String>)> {
    let method = mcall.method.to_string();
    if !is_method_router_constructor(&method) {
        return None;
    }
    let handler_arg = mcall.args.first()?;
    let handler_path = expr_to_path_segments(handler_arg)?;
    Some((method, handler_path))
}

fn is_method_router_constructor(name: &str) -> bool {
    matches!(
        name,
        "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "trace" | "any"
    )
}

fn expr_to_path_segments(expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::Path(p) => Some(
            p.path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect(),
        ),
        // `get(handler.into())` etc. — unwrap one method-call layer.
        Expr::MethodCall(m) => expr_to_path_segments(&m.receiver),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Use-item collection
// ---------------------------------------------------------------------------

fn collect_use_items(
    tree: &UseTree,
    prefix: &[String],
    out: &mut HashMap<String, String>,
    crate_name: &str,
) {
    match tree {
        UseTree::Path(p) => {
            let mut new_prefix = prefix.to_vec();
            new_prefix.push(p.ident.to_string());
            collect_use_items(&p.tree, &new_prefix, out, crate_name);
        }
        UseTree::Name(n) => {
            let ident = n.ident.to_string();
            let mut full = prefix.to_vec();
            full.push(ident.clone());
            out.insert(ident, expand_use_path(&full, crate_name));
        }
        UseTree::Rename(r) => {
            let alias = r.rename.to_string();
            let mut full = prefix.to_vec();
            full.push(r.ident.to_string());
            out.insert(alias, expand_use_path(&full, crate_name));
        }
        UseTree::Group(g) => {
            for item in &g.items {
                collect_use_items(item, prefix, out, crate_name);
            }
        }
        UseTree::Glob(_) => {
            // Glob imports are unresolvable to specific idents without
            // following the wildcarded module's exports — out of scope
            // for the producer. Caller will fall through to module-local
            // resolution, which is correct for the common case where
            // `use crate::foo::*;` exports handler fns living in
            // `crate::foo` AND the route declaration is also in
            // `crate::foo` (rare).
        }
    }
}

/// Expand a `use` path to a fully-qualified Rust path under the file's crate.
fn expand_use_path(segments: &[String], crate_name: &str) -> String {
    if segments.is_empty() {
        return String::new();
    }
    let head = segments[0].as_str();
    let tail = &segments[1..];
    let mut out = match head {
        "crate" => crate_name.to_string(),
        // `use self::x::y` — best-effort: drop `self::` and let the path
        // start with whatever follows. Resolving against the file's
        // module path is more accurate but `self::` in `use` is rare.
        "self" => String::new(),
        // External-crate or absolute path — keep as-is.
        other => other.to_string(),
    };
    for seg in tail {
        if !out.is_empty() {
            out.push_str("::");
        }
        out.push_str(seg);
    }
    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a syn AST node's span to a 1-based line number. Avoids
/// depending on `proc_macro2` directly — `Span::start()` is reachable via
/// the `syn::spanned::Spanned` trait.
fn span_line<S: syn::spanned::Spanned>(node: &S) -> u32 {
    u32::try_from(node.span().start().line).unwrap_or(0)
}

fn extract_string_literal(expr: &Expr) -> Option<String> {
    if let Expr::Lit(l) = expr
        && let Lit::Str(s) = &l.lit
    {
        return Some(s.value());
    }
    None
}

fn join_url_path(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        return suffix.to_string();
    }
    if suffix.is_empty() || suffix == "/" {
        return prefix.to_string();
    }
    let p = prefix.trim_end_matches('/');
    let s = suffix.strip_prefix('/').unwrap_or(suffix);
    format!("{p}/{s}")
}

/// Compute the file's module path (e.g. `mokumo_shop::user::handler`)
/// from its absolute path under `crates/<crate>/src/`.
///
/// Conventions:
/// - `src/lib.rs` / `src/main.rs` → `<crate_name>`.
/// - `src/foo.rs` → `<crate_name>::foo`.
/// - `src/foo/mod.rs` → `<crate_name>::foo`.
/// - `src/foo/bar.rs` → `<crate_name>::foo::bar`.
fn file_module_path(crate_name: &str, src_dir: &Path, file: &Path) -> String {
    let Ok(rel) = file.strip_prefix(src_dir) else {
        return crate_name.to_string();
    };
    let parts: Vec<_> = rel.components().collect();
    let mut segments: Vec<String> = Vec::new();
    for (i, comp) in parts.iter().enumerate() {
        let s = comp.as_os_str().to_string_lossy();
        let is_last = i + 1 == parts.len();
        if let Some(seg) = file_path_segment(&s, is_last) {
            segments.push(seg);
        }
    }
    let segs_owned: Vec<String> = segments;
    join_path(crate_name, &segs_owned)
}

/// Translate one rel-path component into an optional module segment.
/// `lib.rs` / `main.rs` / `mod.rs` contribute nothing (they ARE the parent
/// module); other `.rs` files contribute their stem; directory components
/// pass through verbatim.
fn file_path_segment(name: &str, is_last: bool) -> Option<String> {
    if !is_last {
        return Some(name.to_string());
    }
    if name == "lib.rs" || name == "main.rs" || name == "mod.rs" {
        return None;
    }
    name.strip_suffix(".rs").map(str::to_string)
}

/// Append `::seg` for each `seg` in `tail` to `prefix`. Centralised so the
/// many call-sites in `resolve_path` and `file_module_path` don't open-code
/// the same loop.
fn join_path(prefix: &str, tail: &[String]) -> String {
    let mut out = prefix.to_string();
    for seg in tail {
        out.push_str("::");
        out.push_str(seg);
    }
    out
}

/// Parent of a `::`-delimited module path, or `""` for a single segment.
fn parent_module(module_path: &str) -> &str {
    module_path.rsplit_once("::").map_or("", |(p, _)| p)
}

// ---------------------------------------------------------------------------
// Prefix resolution (callgraph walk)
// ---------------------------------------------------------------------------

fn resolve_prefixes(
    builders: &HashMap<String, BuilderFn>,
    edges: &[NestEdge],
) -> HashMap<String, String> {
    // Inverse map: child_builder → list of (parent, prefix). A child can
    // be nested under multiple parents; we take the first one we encounter
    // and report it.
    let mut parent_of: HashMap<&str, (&str, &str)> = HashMap::new();
    for e in edges {
        let Some(child) = e.child.as_deref() else {
            continue;
        };
        parent_of
            .entry(child)
            .or_insert_with(|| (e.parent.as_str(), e.prefix.as_str()));
    }

    // For each builder, walk up the parent chain accumulating prefixes.
    // Cap at builders.len() iterations to break any cycle defensively.
    let mut out: HashMap<String, String> = HashMap::new();
    for builder_path in builders.keys() {
        let mut prefix = String::new();
        let mut current = builder_path.as_str();
        let mut steps = 0usize;
        let max_steps = builders.len().saturating_add(1);
        while let Some((parent, p)) = parent_of.get(current).copied() {
            // Prepend this nest's prefix to the accumulating string.
            prefix = join_url_path(p, &prefix);
            current = parent;
            steps += 1;
            if steps > max_steps {
                // Cycle in the nest graph — bail with whatever prefix we
                // have so far. The producer will surface this as
                // unresolvable indirectly via a route diagnostic.
                break;
            }
        }
        out.insert(builder_path.clone(), prefix);
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_crate(root: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = root.join(name);
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        for (path, body) in files {
            let abs = src.join(path);
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(abs, body).unwrap();
        }
        dir
    }

    #[test]
    fn walks_simple_top_level_router() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                r#"
use axum::{Router, routing::{get, post}};
pub fn router() -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/users", post(crate::user::create))
}
fn health() {}
                "#,
            )],
        );
        let outcome = walk(&[("demo".to_string(), dir)]).unwrap();
        let routes: Vec<(String, String, String)> = outcome
            .routes
            .iter()
            .map(|r| (r.method.clone(), r.path.clone(), r.rust_path.clone()))
            .collect();
        assert!(
            routes.contains(&("GET".into(), "/api/health".into(), "demo::health".into())),
            "routes = {routes:?}"
        );
        assert!(
            routes.contains(&(
                "POST".into(),
                "/api/users".into(),
                "demo::user::create".into()
            )),
            "routes = {routes:?}"
        );
    }

    #[test]
    fn walks_chained_methods_per_route() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                r#"
use axum::{Router, routing::{get, post}};
pub fn router() -> Router {
    Router::new()
        .route("/x", get(crate::a).post(crate::b))
}
                "#,
            )],
        );
        let outcome = walk(&[("demo".to_string(), dir)]).unwrap();
        let methods: Vec<&str> = outcome.routes.iter().map(|r| r.method.as_str()).collect();
        assert!(methods.contains(&"GET"));
        assert!(methods.contains(&"POST"));
    }

    #[test]
    fn nests_qualified_builder_inherits_prefix() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[
                (
                    "lib.rs",
                    r#"
use axum::Router;
pub fn router() -> Router {
    Router::new().nest("/api/users", crate::user::router())
}
                    "#,
                ),
                (
                    "user.rs",
                    r#"
use axum::{Router, routing::get};
pub fn router() -> Router {
    Router::new().route("/", get(list)).route("/{id}", get(show))
}
fn list() {}
fn show() {}
                    "#,
                ),
            ],
        );
        let outcome = walk(&[("demo".to_string(), dir)]).unwrap();
        let paths: Vec<String> = outcome.routes.iter().map(|r| r.path.clone()).collect();
        assert!(
            paths.contains(&"/api/users".to_string()),
            "paths = {paths:?}"
        );
        assert!(
            paths.contains(&"/api/users/{id}".to_string()),
            "paths = {paths:?}"
        );
    }

    #[test]
    fn resolves_bare_handler_via_use_item() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                r#"
use axum::{Router, routing::get};
use crate::health_check::health;
pub fn router() -> Router {
    Router::new().route("/h", get(health))
}
                "#,
            )],
        );
        let outcome = walk(&[("demo".to_string(), dir)]).unwrap();
        let r = outcome
            .routes
            .iter()
            .find(|r| r.path == "/h")
            .expect("route not found");
        assert_eq!(r.rust_path, "demo::health_check::health");
    }

    #[test]
    fn resolves_sibling_module_handler_via_mod_decl() {
        // Repro for the case in `crates/kikan/src/platform/v1/auth/mod.rs`:
        // `pub mod login;` declares a sibling module, then the router does
        // `.post(login::login)`. Without `module_decls` tracking, the
        // multi-segment head `login` would fall through to the
        // "qualified-by-crate-name" branch and emit literal `login::login`.
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[
                (
                    "auth/mod.rs",
                    r#"
use axum::{Router, routing::post};
pub mod login;
pub fn router() -> Router {
    Router::new().route("/api/auth/login", post(login::login))
}
                    "#,
                ),
                (
                    "auth/login.rs",
                    "
pub async fn login() {}
                    ",
                ),
            ],
        );
        let outcome = walk(&[("demo".to_string(), dir)]).unwrap();
        let r = outcome
            .routes
            .iter()
            .find(|r| r.path == "/api/auth/login")
            .expect("route not found");
        assert_eq!(r.rust_path, "demo::auth::login::login");
    }

    #[test]
    fn resolves_use_with_alias() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                r#"
use axum::{Router, routing::get};
use crate::a::actual_handler as h;
pub fn router() -> Router { Router::new().route("/x", get(h)) }
                "#,
            )],
        );
        let outcome = walk(&[("demo".to_string(), dir)]).unwrap();
        let r = outcome.routes.iter().find(|r| r.path == "/x").unwrap();
        assert_eq!(r.rust_path, "demo::a::actual_handler");
    }

    #[test]
    fn inline_anonymous_nest_picks_up_prefix() {
        let tmp = tempdir().unwrap();
        let dir = write_crate(
            tmp.path(),
            "demo",
            &[(
                "lib.rs",
                r#"
use axum::{Router, routing::get};
pub fn router() -> Router {
    Router::new()
        .nest("/api/v1", Router::new().route("/users", get(list)))
}
fn list() {}
                "#,
            )],
        );
        let outcome = walk(&[("demo".to_string(), dir)]).unwrap();
        let paths: Vec<String> = outcome.routes.iter().map(|r| r.path.clone()).collect();
        assert!(
            paths.contains(&"/api/v1/users".to_string()),
            "paths = {paths:?}"
        );
    }

    #[test]
    fn module_path_lib_rs_is_crate_root() {
        let p = file_module_path("demo", Path::new("/x/src"), Path::new("/x/src/lib.rs"));
        assert_eq!(p, "demo");
    }

    #[test]
    fn module_path_nested_module() {
        let p = file_module_path(
            "demo",
            Path::new("/x/src"),
            Path::new("/x/src/user/handler.rs"),
        );
        assert_eq!(p, "demo::user::handler");
    }

    #[test]
    fn module_path_mod_rs_is_directory_module() {
        let p = file_module_path("demo", Path::new("/x/src"), Path::new("/x/src/user/mod.rs"));
        assert_eq!(p, "demo::user");
    }

    #[test]
    fn join_url_path_handles_edges() {
        assert_eq!(join_url_path("", "/a"), "/a");
        assert_eq!(join_url_path("/a", "/"), "/a");
        assert_eq!(join_url_path("/a", ""), "/a");
        assert_eq!(join_url_path("/a", "/b"), "/a/b");
        assert_eq!(join_url_path("/a/", "/b"), "/a/b");
        assert_eq!(join_url_path("/a", "b"), "/a/b");
    }
}
