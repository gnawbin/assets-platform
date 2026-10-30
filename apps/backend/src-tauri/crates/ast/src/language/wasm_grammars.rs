//! Table of the WebAssembly grammars published by
//! [stencil-hq/wasm-grammars](https://github.com/stencil-hq/wasm-grammars).
//!
//! Languages whose grammar is linked into the addon live in [`super::parsers`]
//! instead; everything else names its entry here through the `wasm` arm of
//! [`grammar!`](super::grammar). [`super::grammar`] downloads `<file>.zst` from
//! [`RELEASE`] into the grammar directory, decompresses it to `<file>` and then
//! reads that module.
//!
//! PROVISIONAL METADATA: `sha256`/`size` are placeholders until the generator
//! that pins this table (`bun run gen:grammars`, see [`super::grammar`]) lands.
//! They must be replaced with the digest and byte size of the decompressed
//! module together with the real release file names; nothing reads them yet, so
//! the placeholder values do not affect parsing.

use super::grammar::WasmGrammar;

/// Release tag every entry below is pinned to.
pub const RELEASE: &str = "v1";

/// Digest of a module whose real SHA-256 is not pinned yet.
const PENDING_SHA256: &str = "";
/// Byte size of a module whose real size is not pinned yet.
const PENDING_SIZE: u64 = 0;

pub static ASTRO: WasmGrammar = WasmGrammar::new(
    "astro",
    "tree_sitter_astro",
    "astro.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static CLOJURE: WasmGrammar = WasmGrammar::new(
    "clojure",
    "tree_sitter_clojure",
    "clojure.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static CMAKE: WasmGrammar = WasmGrammar::new(
    "cmake",
    "tree_sitter_cmake",
    "cmake.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static CSHARP: WasmGrammar = WasmGrammar::new(
    "csharp",
    "tree_sitter_c_sharp",
    "csharp.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static DART: WasmGrammar = WasmGrammar::new(
    "dart",
    "tree_sitter_dart",
    "dart.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static DOCKERFILE: WasmGrammar = WasmGrammar::new(
    "dockerfile",
    "tree_sitter_dockerfile",
    "dockerfile.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static ELIXIR: WasmGrammar = WasmGrammar::new(
    "elixir",
    "tree_sitter_elixir",
    "elixir.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static EMACS_LISP: WasmGrammar = WasmGrammar::new(
    "emacs-lisp",
    "tree_sitter_elisp",
    "emacs-lisp.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static ERLANG: WasmGrammar = WasmGrammar::new(
    "erlang",
    "tree_sitter_erlang",
    "erlang.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static FORTRAN: WasmGrammar = WasmGrammar::new(
    "fortran",
    "tree_sitter_fortran",
    "fortran.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static GRAPHQL: WasmGrammar = WasmGrammar::new(
    "graphql",
    "tree_sitter_graphql",
    "graphql.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static HASKELL: WasmGrammar = WasmGrammar::new(
    "haskell",
    "tree_sitter_haskell",
    "haskell.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static HCL: WasmGrammar = WasmGrammar::new(
    "hcl",
    "tree_sitter_hcl",
    "hcl.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static INI: WasmGrammar = WasmGrammar::new(
    "ini",
    "tree_sitter_ini",
    "ini.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static JULIA: WasmGrammar = WasmGrammar::new(
    "julia",
    "tree_sitter_julia",
    "julia.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static JUST: WasmGrammar = WasmGrammar::new(
    "just",
    "tree_sitter_just",
    "just.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static KOTLIN: WasmGrammar = WasmGrammar::new(
    "kotlin",
    "tree_sitter_kotlin",
    "kotlin.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static LUA: WasmGrammar = WasmGrammar::new(
    "lua",
    "tree_sitter_lua",
    "lua.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static MAKE: WasmGrammar = WasmGrammar::new(
    "make",
    "tree_sitter_make",
    "make.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static NIX: WasmGrammar = WasmGrammar::new(
    "nix",
    "tree_sitter_nix",
    "nix.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static OBJC: WasmGrammar = WasmGrammar::new(
    "objc",
    "tree_sitter_objc",
    "objc.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static OCAML: WasmGrammar = WasmGrammar::new(
    "ocaml",
    "tree_sitter_ocaml",
    "ocaml.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static ODIN: WasmGrammar = WasmGrammar::new(
    "odin",
    "tree_sitter_odin",
    "odin.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static PHP: WasmGrammar = WasmGrammar::new(
    "php",
    "tree_sitter_php",
    "php.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static POWERSHELL: WasmGrammar = WasmGrammar::new(
    "powershell",
    "tree_sitter_powershell",
    "powershell.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static PROTOBUF: WasmGrammar = WasmGrammar::new(
    "protobuf",
    "tree_sitter_proto",
    "protobuf.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static R: WasmGrammar =
    WasmGrammar::new("r", "tree_sitter_r", "r.wasm", PENDING_SHA256, PENDING_SIZE);

pub static RUBY: WasmGrammar = WasmGrammar::new(
    "ruby",
    "tree_sitter_ruby",
    "ruby.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static SCALA: WasmGrammar = WasmGrammar::new(
    "scala",
    "tree_sitter_scala",
    "scala.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static SOLIDITY: WasmGrammar = WasmGrammar::new(
    "solidity",
    "tree_sitter_solidity",
    "solidity.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static SQL: WasmGrammar = WasmGrammar::new(
    "sql",
    "tree_sitter_sql",
    "sql.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static STARLARK: WasmGrammar = WasmGrammar::new(
    "starlark",
    "tree_sitter_starlark",
    "starlark.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static SVELTE: WasmGrammar = WasmGrammar::new(
    "svelte",
    "tree_sitter_svelte",
    "svelte.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static SWIFT: WasmGrammar = WasmGrammar::new(
    "swift",
    "tree_sitter_swift",
    "swift.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static TLAPLUS: WasmGrammar = WasmGrammar::new(
    "tlaplus",
    "tree_sitter_tlaplus",
    "tlaplus.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static VERILOG: WasmGrammar = WasmGrammar::new(
    "verilog",
    "tree_sitter_verilog",
    "verilog.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static VUE: WasmGrammar = WasmGrammar::new(
    "vue",
    "tree_sitter_vue",
    "vue.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static XML: WasmGrammar = WasmGrammar::new(
    "xml",
    "tree_sitter_xml",
    "xml.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

pub static ZIG: WasmGrammar = WasmGrammar::new(
    "zig",
    "tree_sitter_zig",
    "zig.wasm",
    PENDING_SHA256,
    PENDING_SIZE,
);

/// Every grammar above, for hosts that enumerate what they can download.
pub const ALL: &[&WasmGrammar] = &[
    &ASTRO,
    &CLOJURE,
    &CMAKE,
    &CSHARP,
    &DART,
    &DOCKERFILE,
    &ELIXIR,
    &EMACS_LISP,
    &ERLANG,
    &FORTRAN,
    &GRAPHQL,
    &HASKELL,
    &HCL,
    &INI,
    &JULIA,
    &JUST,
    &KOTLIN,
    &LUA,
    &MAKE,
    &NIX,
    &OBJC,
    &OCAML,
    &ODIN,
    &PHP,
    &POWERSHELL,
    &PROTOBUF,
    &R,
    &RUBY,
    &SCALA,
    &SOLIDITY,
    &SQL,
    &STARLARK,
    &SVELTE,
    &SWIFT,
    &TLAPLUS,
    &VERILOG,
    &VUE,
    &XML,
    &ZIG,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_grammar_is_described_once() {
        for grammar in ALL {
            assert!(
                !grammar.language.is_empty(),
                "{} has no language name",
                grammar.file
            );
            assert!(
                grammar.symbol.starts_with("tree_sitter_"),
                "{} exports {} instead of a tree_sitter_ symbol",
                grammar.file,
                grammar.symbol
            );
            assert!(
                grammar.file.ends_with(".wasm"),
                "{} is not a module file name",
                grammar.file
            );
        }
        let mut files: Vec<&str> = ALL.iter().map(|grammar| grammar.file).collect();
        let total = files.len();
        files.sort_unstable();
        files.dedup();
        assert_eq!(files.len(), total, "two grammars share a module file name");
    }
}
