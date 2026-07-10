// Verifies the curated per-keyword value trimming under the `lynx` feature:
// `display`, `overflow{,-x,-y}` and `white-space` are supported properties, but
// Lynx accepts only a subset of their standard keyword values (verified against
// core/renderer/css/parser/enum_handler.cc). The Lynx-accepted keywords must
// parse; the rest must be rejected. Gated behind `lynx`.
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
fn display_keyword_gating() {
    // Lynx: none | flex | grid | block, plus the Lynx-only linear | relative.
    // `grid` is force-enabled under `lynx` (grid_enabled() returns true), so it
    // parses out of the box — matching its grid-* longhands, which are enabled
    // the same way — without needing stylo's `layout.grid.enabled` pref.
    for value in ["none", "flex", "grid", "block", "linear", "relative"] {
        assert_accepts("display", value);
    }
    // Everything else in stylo's `display` grammar is gated out.
    for value in [
        "inline",
        "inline-block",
        "inline-flex",
        "inline-grid",
        "inline-table",
        "flow-root",
        "list-item",
        "contents",
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
    // Lynx overflow / overflow-x / overflow-y: visible | hidden | scroll.
    for prop in ["overflow", "overflow-x", "overflow-y"] {
        for value in ["visible", "hidden", "scroll"] {
            assert_accepts(prop, value);
        }
        for value in ["auto", "clip", "overlay"] {
            assert_rejects(prop, value);
        }
    }
}

#[test]
fn white_space_keyword_gating() {
    // Lynx white-space: normal | nowrap (nothing else).
    for value in ["normal", "nowrap"] {
        assert_accepts("white-space", value);
    }
    // Everything else is gated out: the legacy `pre*` keywords, and — since the
    // component-longhand fallback is disabled under `lynx` — the bare
    // `text-wrap-mode` / `white-space-collapse` values and their combinations.
    for value in [
        "pre",
        "pre-wrap",
        "pre-line",
        "break-spaces",
        "wrap",
        "collapse",
        "preserve",
        "preserve-breaks",
        "nowrap preserve",
    ] {
        assert_rejects("white-space", value);
    }
}
