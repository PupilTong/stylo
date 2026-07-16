//! Locks Lynx containment hints to Stylo's upstream structural bit layout.
#![cfg(feature = "lynx")]

use cssparser::{Parser as CssParser, ParserInput};
use style::context::QuirksMode;
use style::custom_properties::AttrTaint;
use style::parser::{Parse, ParserContext};
use style::stylesheets::{Origin, UrlExtraData};
use style::values::specified::box_::{Contain, WillChange};
use style_traits::ParsingMode;

fn with_parser<T>(css: &str, parse: impl FnOnce(&ParserContext, &mut CssParser<'_, '_>) -> T) -> T {
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
    parse(&context, &mut parser)
}

fn contain_bits(css: &str) -> u8 {
    with_parser(css, |context, parser| {
        parser
            .parse_entirely(|input| Contain::parse(context, input))
            .unwrap()
            .bits()
    })
}

fn will_change_bits(css: &str) -> u16 {
    with_parser(css, |context, parser| {
        parser
            .parse_entirely(|input| WillChange::parse(context, input))
            .unwrap()
            .bits
            .bits()
    })
}

#[test]
fn contain_uses_upstream_structural_bits() {
    assert_eq!(contain_bits("none"), 0);
    assert_eq!(contain_bits("inline-size"), 1 << 0);
    assert_eq!(contain_bits("layout"), 1 << 2);
    assert_eq!(contain_bits("style"), 1 << 3);
    assert_eq!(contain_bits("paint"), 1 << 4);
    assert_eq!(contain_bits("size"), (1 << 5) | (1 << 1) | (1 << 0));
    assert_eq!(
        contain_bits("content"),
        (1 << 6) | (1 << 4) | (1 << 3) | (1 << 2)
    );
    assert_eq!(
        contain_bits("strict"),
        (1 << 7) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2) | (1 << 1) | (1 << 0)
    );
}

#[test]
fn will_change_contain_uses_upstream_structural_bit() {
    assert_eq!(will_change_bits("contain"), 1 << 3);
    assert_eq!(will_change_bits("contents"), 0);
    assert_eq!(will_change_bits("opacity"), (1 << 10) | (1 << 4));
}
