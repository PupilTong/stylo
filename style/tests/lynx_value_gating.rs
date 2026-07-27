// Verifies the curated per-keyword value trimming under the `lynx` feature:
// `display` and `overflow{,-x,-y}` use Lynx value sets. `white-space` is a
// supported shorthand and therefore keeps Stylo's complete shorthand grammar,
// together with both component longhands.
#![cfg(feature = "lynx")]

use style::context::QuirksMode;
use style::properties::declaration_block::parse_one_declaration_into;
use style::properties::{PropertyId, SourcePropertyDeclaration};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style_traits::ParsingMode;

fn url_data() -> UrlExtraData {
    UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap())
}

/// Parse `name: value` through the normal content path (the property itself
/// must be content-enabled, or this returns `Err`).
fn parse_value(name: &str, value: &str) -> Result<SourcePropertyDeclaration, ()> {
    let url_data = url_data();
    let id = PropertyId::parse_enabled_for_all_content(name)?;
    let mut declarations = SourcePropertyDeclaration::default();
    parse_one_declaration_into(
        &mut declarations,
        id,
        value,
        Origin::Author,
        &url_data,
        None,
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    )?;
    Ok(declarations)
}

fn assert_accepts(name: &str, value: &str) {
    assert!(
        parse_value(name, value).is_ok(),
        "`{name}: {value}` is a Lynx-supported value and must parse under the `lynx` feature",
    );
}

fn assert_rejects(name: &str, value: &str) {
    assert!(
        parse_value(name, value).is_err(),
        "`{name}: {value}` is not a Lynx value and must be rejected under the `lynx` feature",
    );
}

#[test]
fn custom_properties_and_var_references_remain_enabled() {
    assert_accepts("--lynx-size", "12px");
    assert_accepts("width", "var(--lynx-size)");
    assert_accepts("width", "var(--missing, 10px)");
}

#[test]
fn display_keyword_gating() {
    // Lynx: none | contents | flex | grid, plus the Lynx-only linear | relative.
    // `grid` is force-enabled under `lynx` (grid_enabled() returns true), so it
    // parses out of the box — matching its grid-* longhands, which are enabled
    // the same way — without needing stylo's `layout.grid.enabled` pref.
    for value in ["none", "contents", "flex", "grid", "linear", "relative"] {
        assert_accepts("display", value);
    }
    // Everything else in stylo's `display` grammar is gated out.
    for value in [
        "inline",
        "block",
        "inline-block",
        "inline-flex",
        "inline-grid",
        "inline-table",
        "flow-root",
        "list-item",
        "table",
        "table-row",
        "table-cell",
        "table-caption",
    ] {
        assert_rejects("display", value);
    }
}

#[test]
fn overflow_keyword_gating() {
    // Lynx overflow / overflow-x / overflow-y: visible | hidden.
    for prop in ["overflow", "overflow-x", "overflow-y"] {
        for value in ["visible", "hidden"] {
            assert_accepts(prop, value);
        }
        for value in ["scroll", "auto", "clip", "overlay"] {
            assert_rejects(prop, value);
        }
    }
}

#[test]
fn white_space_uses_the_complete_shorthand_grammar() {
    for value in [
        "normal",
        "nowrap",
        "pre",
        "pre-wrap",
        "pre-line",
        "break-spaces",
        "preserve nowrap",
    ] {
        assert_accepts("white-space", value);
    }
    assert_accepts("text-wrap-mode", "wrap");
    assert_accepts("text-wrap-mode", "nowrap");
    assert_accepts("white-space-collapse", "preserve-breaks");
    assert_rejects("white-space", "balance");
}

#[test]
fn css_wide_keywords_keep_their_standard_meaning() {
    for property in ["display", "width", "color", "background", "font-size"] {
        for value in ["inherit", "initial", "unset", "revert", "revert-layer"] {
            assert_accepts(property, value);
        }
    }
}

#[test]
fn direction_follows_the_w3c_grammar() {
    for value in ["ltr", "rtl"] {
        assert_accepts("direction", value);
    }
    assert_rejects("direction", "normal");
    assert_rejects("direction", "lynx-rtl");
}

#[test]
fn background_clip_accepts_the_lynx_value_surface() {
    // Lynx's background-clip surface: the three standard boxes, the
    // backgrounds-4 `text` value (Core in Lynx — also the lowering target
    // for Lynx's gradient-valued `color` sugar), and the Lynx-only
    // `border-area` (v3.6).
    for value in ["border-box", "padding-box", "content-box", "text", "border-area"] {
        assert_accepts("background-clip", value);
    }
    // mask-clip keeps rejecting the background-only values.
    assert_rejects("mask-clip", "text");
    assert_rejects("mask-clip", "border-area");
}

#[test]
fn outline_accepts_the_w3c_grammar_without_offset() {
    // The lynx grammar seeds `outline`/`outline-color`/`outline-style`/
    // `outline-width` (Lynx Core rows); `outline-offset` stays out — Lynx
    // outlines are flush rings (lynx/core/style/outline_data.h has no
    // offset field).
    assert_accepts("outline", "1px solid red");
    assert_accepts("outline", "medium auto");
    assert_accepts("outline-style", "auto");
    assert_accepts("outline-style", "dashed");
    assert_accepts("outline-width", "thick");
    assert_accepts("outline-color", "rebeccapurple");
    // The lynx color grammar has no `currentcolor` keyword (Lynx's own
    // color parser: keyword/hex/rgb/rgba/hsl/hsla) — outline-color follows
    // the shared trim; its *initial* value is still the currentcolor
    // computed value internally.
    assert_rejects("outline-color", "currentcolor");
    assert_rejects("outline-offset", "2px");
}
