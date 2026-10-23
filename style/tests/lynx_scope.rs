//! css-cascade-6 §2.5 scoped styles under the `lynx` feature: the `@scope`
//! rule parses (it is pref-gated off in a stock Servo build, see
//! `lynx_feature_off_parity.rs`).
//!
//! Ports of the CSSOM-observable halves of the web-platform-tests in
//! `css/css-cascade/`:
//!
//! - `at-scope-parsing.html` in full: each `test_valid` prelude serializes
//!   back to the expected text, each `test_invalid` prelude yields no rule.
//! - `at-scope-relative-syntax.html`, the "string" creation method: a
//!   relative inner selector under `@scope` serializes with an explicit `&`,
//!   and under a style rule's nested `@scope` stays relative. The
//!   `insertRule` method is a CSSOM mutation API this crate does not carry.
//! - `scope-declarations.html`, the `CSSNestedDeclarations` cases:
//!   declarations directly inside `@scope` become nested-declarations rules
//!   in source order between the style rules.
//!
//! Plus `@scope` nested inside a style rule parses (css-nesting "Nesting
//! @scope").
//!
//! Matching, proximity and invalidation are exercised by the outer engine's
//! `crates/dom/tests/scope.rs`.
#![cfg(feature = "lynx")]

use cssparser::ToCss;
use servo_arc::Arc;
use style::context::QuirksMode;
use style::media_queries::MediaList;
use style::shared_lock::{SharedRwLock, SharedRwLockReadGuard, ToCssWithGuard};
use style::stylesheets::{
    AllowImportRules, CssRule, CssRules, Origin, Stylesheet, StylesheetInDocument, UrlExtraData,
};

fn url_data() -> UrlExtraData {
    UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap())
}

fn sheet(css: &str, lock: &SharedRwLock) -> Stylesheet {
    Stylesheet::from_str(
        css,
        url_data(),
        Origin::Author,
        Arc::new(lock.wrap(MediaList::empty())),
        lock.clone(),
        None,
        None,
        QuirksMode::NoQuirks,
        AllowImportRules::Yes,
    )
}

/// The top-level rules of `css`, serialized one per entry.
fn top_level(css: &str) -> Vec<String> {
    let lock = SharedRwLock::new();
    let sheet = sheet(css, &lock);
    let guard = lock.read();
    sheet
        .contents(&guard)
        .rules(&guard)
        .iter()
        .map(|rule| serialize(rule, &guard))
        .collect()
}

fn serialize(rule: &CssRule, guard: &SharedRwLockReadGuard) -> String {
    let mut out = String::new();
    rule.to_css(guard, &mut out).unwrap();
    out
}

/// The child rules of a rule that has a block.
fn children<'a>(rule: &'a CssRule, guard: &'a SharedRwLockReadGuard) -> &'a CssRules {
    match rule {
        CssRule::Scope(scope) => scope.rules.read_with(guard),
        CssRule::Style(style) => style
            .read_with(guard)
            .rules
            .as_ref()
            .expect("a style rule with nested rules")
            .read_with(guard),
        CssRule::Media(media) => media.rules.read_with(guard),
        other => panic!("rule without children: {other:?}"),
    }
}

fn selector_text(rule: &CssRule, guard: &SharedRwLockReadGuard) -> String {
    match rule {
        CssRule::Style(style) => style.read_with(guard).selectors.to_css_string(),
        other => panic!("not a style rule: {other:?}"),
    }
}

// WPT css/css-cascade/at-scope-parsing.html, `test_valid`.
#[test]
fn wpt_at_scope_parsing_valid() {
    for (actual, expected) in [
        ("@scope (.a)", "@scope (.a)"),
        ("@scope (.a + .b)", "@scope (.a + .b)"),
        ("@scope (.a:hover)", "@scope (.a:hover)"),
        ("@scope (.a:hover, #b, div)", "@scope (.a:hover, #b, div)"),
        ("@scope (:is(div, span))", "@scope (:is(div, span))"),
        ("@scope (.a) to (.b)", "@scope (.a) to (.b)"),
        ("@scope (.a)to (.b)", "@scope (.a) to (.b)"),
        (
            "@scope (.a) to (.b:hover, #c, div)",
            "@scope (.a) to (.b:hover, #c, div)",
        ),
        ("@scope", "@scope"),
        ("@scope to (.a)", "@scope to (.a)"),
        ("@scope (.a) to (&)", "@scope (.a) to (&)"),
        ("@scope (.a) to (& > &)", "@scope (.a) to (& > &)"),
        ("@scope (.a) to (> .b)", "@scope (.a) to (> .b)"),
        ("@scope (.a) to (+ .b)", "@scope (.a) to (+ .b)"),
        ("@scope (.a) to (~ .b)", "@scope (.a) to (~ .b)"),
    ] {
        let rules = top_level(&format!("{actual}{{}}"));
        assert_eq!(rules.len(), 1, "`{actual}` must yield one rule");
        assert_eq!(
            rules[0],
            format!("{expected} {{\n}}"),
            "`{actual}` serializes like Firefox and Chrome"
        );
    }
}

// WPT css/css-cascade/at-scope-parsing.html, `test_invalid`.
#[test]
fn wpt_at_scope_parsing_invalid() {
    for actual in [
        "@scope ()",
        "@scope to ()",
        "@scope () to ()",
        "@scope (.c <> .d)",
        "@scope (.a, .c <> .d)",
        "@scope (.a <> .b, .c)",
        "@scope (div::before)",
        "@scope (div::after)",
        "@scope (slotted(div))",
        "@scope (.a) to (div::before)",
        "@scope (> &) to (>>)",
        "@scope div",
        "@scope (.a) unknown (.c)",
        "@scope (.a) to unknown (.c)",
        "@scope (.a) 1px (.c)",
        "@scope (.a) to unknown(c)",
        "@scope unknown(.a)",
        "@scope 1px",
        "@scope creep",
        "@scope )))",
        "@scope (",
        "@scope ( {}",
        "@scope to",
        "@scope }",
        "@scope (.a",
        "@scope (.a to (.b)",
        "@scope ( to (.b)",
        "@scope (.a) from (.c)",
    ] {
        let rules = top_level(&format!("{actual}{{}}"));
        assert!(
            rules.is_empty(),
            "`{actual}` is invalid in Firefox and Chrome, got {rules:?}"
        );
    }
}

/// The selector text of the innermost rule of `prelude { ... { inner {} } }`,
/// asserting every level holds exactly one rule (WPT `innermost_selector`).
fn innermost_selector(prelude: &[&str], inner: &str) -> String {
    let mut css = format!("{inner} {{}}");
    for p in prelude.iter().rev() {
        css = format!("{p} {{ {css} }}");
    }
    let lock = SharedRwLock::new();
    let sheet = sheet(&css, &lock);
    let guard = lock.read();
    let mut rules = sheet.contents(&guard).rules(&guard);
    assert_eq!(rules.len(), 1, "`{css}`");
    let mut rule = &rules[0];
    for _ in prelude {
        rules = &children(rule, &guard).0;
        assert_eq!(rules.len(), 1, "`{css}`");
        rule = &rules[0];
    }
    selector_text(rule, &guard)
}

// WPT css/css-cascade/at-scope-relative-syntax.html, method "string".
#[test]
fn wpt_at_scope_relative_syntax_string() {
    for (prelude, expected) in [
        (&["@scope", ".nest"][..], "& > .foo"),
        (&[".nest", "@scope"][..], "> .foo"),
        (&["@scope", ".nest", "@media screen"][..], "& > .foo"),
        (&[".nest", "@scope", "@media screen"][..], "> .foo"),
    ] {
        assert_eq!(
            innermost_selector(prelude, "> .foo"),
            expected,
            "`> .foo` in {prelude:?} serializes like Firefox and Chrome"
        );
    }
}

// WPT css/css-cascade/scope-declarations.html, "Declarations are parsed into
// CSSNestedDeclarations, prelude=(.a)" and "prelude=".
#[test]
fn wpt_scope_declarations_nested_declarations() {
    for prelude in ["(.a)", ""] {
        let css = format!(
            "@scope {prelude} {{
                color: red;
                width: 1px;
                .b {{}}
                left: 2px;
                right: 3px;
                .c {{}}
                top: 4px;
                bottom: 5px;
            }}"
        );
        let lock = SharedRwLock::new();
        let sheet = sheet(&css, &lock);
        let guard = lock.read();
        let rules = sheet.contents(&guard).rules(&guard);
        assert_eq!(rules.len(), 1);
        let inner = &children(&rules[0], &guard).0;
        assert_eq!(inner.len(), 5, "prelude `{prelude}`");
        for (index, expected) in [
            (0, "color: red; width: 1px;"),
            (2, "left: 2px; right: 3px;"),
            (4, "top: 4px; bottom: 5px;"),
        ] {
            let CssRule::NestedDeclarations(ref declarations) = inner[index] else {
                panic!(
                    "rule {index} must be nested declarations, got {:?}",
                    inner[index]
                );
            };
            let mut text = String::new();
            declarations
                .read_with(&guard)
                .block
                .read_with(&guard)
                .to_css(&mut text)
                .unwrap();
            assert_eq!(text, expected, "prelude `{prelude}`, rule {index}");
        }
        for index in [1, 3] {
            assert!(
                matches!(inner[index], CssRule::Style(_)),
                "rule {index} must be a style rule"
            );
        }
    }
}

// css-nesting "Nesting @scope": `@scope` is a conditional group rule inside a
// style rule, its `&` the parent rule's selector.
#[test]
fn scope_nests_inside_a_style_rule() {
    // A `<scope-start>` nested in a style rule is relative to the parent
    // rule's selector; stylo writes the implied `&` into the prelude. No WPT
    // case pins this serialization, so it is recorded as observed.
    let rules = top_level(".parent { @scope (.a) to (.b) { .c { color: red } } }");
    assert_eq!(
        rules,
        [".parent {\n  @scope (& .a) to (.b) {\n  .c { color: red; }\n}\n}"],
    );

    let lock = SharedRwLock::new();
    let sheet = sheet(
        ".parent { color: blue; @scope (& > .a) { color: red; .c { color: green } } }",
        &lock,
    );
    let guard = lock.read();
    let rules = sheet.contents(&guard).rules(&guard);
    assert_eq!(rules.len(), 1);
    let nested = &children(&rules[0], &guard).0;
    assert_eq!(nested.len(), 1);
    let CssRule::Scope(ref scope) = nested[0] else {
        panic!("the nested rule must be @scope, got {:?}", nested[0]);
    };
    assert_eq!(
        scope.bounds.start.as_ref().unwrap().to_css_string(),
        "& > .a",
        "the scope start keeps `&`, the parent rule's selector"
    );
    let inner = &scope.rules.read_with(&guard).0;
    assert_eq!(inner.len(), 2);
    assert!(matches!(inner[0], CssRule::NestedDeclarations(_)));
    assert!(matches!(inner[1], CssRule::Style(_)));
}
