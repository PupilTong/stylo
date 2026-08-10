// display:linear|relative are gated behind the `lynx` cargo feature; without it
// there is nothing to test.
#![cfg(feature = "lynx")]

use cssparser::{Parser as CssParser, ParserInput};
use style::context::QuirksMode;
use style::custom_properties::AttrTaint;
use style::parser::{Parse, ParserContext};
use style::stylesheets::{Origin, UrlExtraData};
use style::values::specified::box_::{Display, DisplayInside, DisplayOutside};
use style_traits::{ParsingMode, SpecifiedValueInfo, ToCss};

fn parse_display(css: &str) -> Result<Display, ()> {
    let url_data = UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap());
    let context = ParserContext::new(
        Origin::Author,
        &url_data,
        None,
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        AttrTaint::default(),
    );
    let mut input = ParserInput::new(css);
    let mut parser = CssParser::new(&mut input);
    parser
        .parse_entirely(|input| Display::parse(&context, input))
        .map_err(|_| ())
}

#[test]
fn parses_and_serializes_lynx_display_keywords() {
    let contents = parse_display("contents").unwrap();
    assert_eq!(contents, Display::Contents);
    assert_eq!(contents.to_css_string(), "contents");

    let linear = parse_display("linear").unwrap();
    assert_eq!(linear, Display::Linear);
    assert_eq!(linear.to_css_string(), "linear");

    let relative = parse_display("relative").unwrap();
    assert_eq!(relative, Display::LynxRelative);
    assert_eq!(relative.to_css_string(), "relative");
}

#[test]
fn parses_and_serializes_inline_layout_keywords() {
    let cases = [
        ("inline-flex", Display::InlineFlex, DisplayInside::Flex),
        ("inline-grid", Display::InlineGrid, DisplayInside::Grid),
        (
            "inline-linear",
            Display::InlineLinear,
            DisplayInside::LynxLinear,
        ),
        (
            "inline-relative",
            Display::InlineRelative,
            DisplayInside::LynxRelative,
        ),
    ];

    for (keyword, expected, inside) in cases {
        let display = parse_display(keyword).unwrap();
        assert_eq!(display, expected);
        assert_eq!(display.outside(), DisplayOutside::Inline);
        assert_eq!(display.inside(), inside);
        assert_eq!(display.to_css_string(), keyword);
    }
}

#[test]
fn display_inside_serializes_lynx_keywords() {
    // `Display::to_css` has explicit arms for the full keywords, but the
    // derived `ToCss` on `DisplayInside` (reachable via the multi-keyword
    // fallback) must agree — it kebab-cases the variant NAME unless
    // overridden with `#[css(keyword = ...)]`, which would leak
    // "lynx-linear"/"lynx-relative".
    assert_eq!(DisplayInside::Contents.to_css_string(), "contents");
    assert_eq!(DisplayInside::LynxLinear.to_css_string(), "linear");
    assert_eq!(DisplayInside::LynxRelative.to_css_string(), "relative");
}

#[test]
fn contents_keeps_upstream_box_generation_and_root_fixup_semantics() {
    let contents = parse_display("contents").unwrap();

    assert_eq!(contents.outside(), DisplayOutside::None);
    assert_eq!(contents.inside(), DisplayInside::Contents);
    assert!(contents.is_contents());
    assert_eq!(contents.equivalent_block_display(false), Display::Contents);

    let root_display = contents.equivalent_block_display(true);
    assert_eq!(root_display.outside(), DisplayOutside::Block);
    assert_eq!(root_display.inside(), DisplayInside::Flow);
    assert_eq!(root_display.to_css_string(), "block");
}

#[test]
fn linear_behaves_like_block_level_flex_container() {
    let display = parse_display("linear").unwrap();

    assert_eq!(display.outside(), DisplayOutside::Block);
    assert_eq!(display.inside(), DisplayInside::LynxLinear);
    assert!(display.is_item_container());
    assert!(!display.is_inline_flow());
    assert_eq!(display.equivalent_block_display(false), Display::Linear);
}

#[test]
fn relative_behaves_like_block_without_becoming_css_block() {
    let display = parse_display("relative").unwrap();

    assert_eq!(display.outside(), DisplayOutside::Block);
    assert_eq!(display.inside(), DisplayInside::LynxRelative);
    assert!(!display.is_item_container());
    assert!(!display.is_inline_flow());
    assert_eq!(
        display.equivalent_block_display(false),
        Display::LynxRelative
    );
}

#[test]
fn blockification_preserves_inline_inner_layout_algorithm() {
    let cases = [
        (Display::InlineFlex, Display::Flex),
        (Display::InlineGrid, Display::Grid),
        (Display::InlineLinear, Display::Linear),
        (Display::InlineRelative, Display::LynxRelative),
    ];

    for (inline, expected_block) in cases {
        let blockified = inline.equivalent_block_display(false);
        assert_eq!(blockified, expected_block);
        assert_eq!(blockified.outside(), DisplayOutside::Block);
        assert_eq!(blockified.inside(), inline.inside());
    }
}

#[test]
fn completion_contains_only_supported_inline_layout_keywords() {
    let mut completions = Vec::new();
    Display::collect_completion_keywords(&mut |keywords| {
        completions.extend_from_slice(keywords);
    });

    for keyword in [
        "inline-flex",
        "inline-grid",
        "inline-linear",
        "inline-relative",
    ] {
        assert!(completions.contains(&keyword));
    }
    for keyword in ["inline", "block", "inline-block", "inline flex"] {
        assert!(!completions.contains(&keyword));
    }
}

#[test]
fn lynx_display_keywords_are_single_keyword_values() {
    assert!(parse_display("inline").is_err());
    assert!(parse_display("block").is_err());
    assert!(parse_display("inline-block").is_err());
    assert!(parse_display("inline flex").is_err());
    assert!(parse_display("inline linear").is_err());
    assert!(parse_display("relative list-item").is_err());
}
