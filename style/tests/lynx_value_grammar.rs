//! Representative end-to-end tests for every Lynx-specific value grammar
//! family. Parsing goes through the same author declaration path used by
//! stylesheets and inline style.
#![cfg(feature = "lynx")]

use style::context::QuirksMode;
use style::properties::declaration_block::parse_one_declaration_into;
use style::properties::{PropertyId, SourcePropertyDeclaration};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style_traits::ParsingMode;

fn url_data() -> UrlExtraData {
    UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap())
}

fn parses(name: &str, value: &str) -> bool {
    let Ok(id) = PropertyId::parse_enabled_for_all_content(name) else {
        return false;
    };
    let mut declarations = SourcePropertyDeclaration::default();
    parse_one_declaration_into(
        &mut declarations,
        id,
        value,
        Origin::Author,
        &url_data(),
        None,
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    )
    .is_ok()
}

fn accepts(name: &str, values: &[&str]) {
    for value in values {
        assert!(parses(name, value), "expected `{name}: {value}` to parse");
    }
}

fn rejects(name: &str, values: &[&str]) {
    for value in values {
        assert!(
            !parses(name, value),
            "expected `{name}: {value}` to be rejected"
        );
    }
}

#[test]
fn sizing_position_and_numeric_grammars() {
    accepts("position", &["relative", "absolute", "fixed", "sticky"]);
    rejects("position", &["static"]);
    accepts("visibility", &["visible", "hidden"]);
    rejects("visibility", &["collapse"]);
    accepts(
        "aspect-ratio",
        &["auto", "1", "16 / 9", "auto 16 / 9", "16 / 9 auto"],
    );
    rejects("aspect-ratio", &["none", "auto auto"]);

    accepts(
        "width",
        &[
            "auto",
            "10px",
            "20%",
            "min-content",
            "max-content",
            "fit-content",
            "fit-content(30px)",
        ],
    );
    rejects("width", &["none", "-1px"]);
    accepts(
        "height",
        &["auto", "10px", "20%", "min-content", "max-content"],
    );
    accepts(
        "min-width",
        &["auto", "0", "10px", "25%", "min-content", "max-content"],
    );
    accepts("min-height", &["auto", "10px", "min-content"]);
    rejects("min-width", &["none", "-1px"]);
    rejects("min-height", &["none", "-1px"]);
    accepts("max-width", &["0", "10px", "25%"]);
    rejects("max-width", &["none", "max-content", "-1px"]);

    accepts("column-gap", &["normal", "0", "10px", "25%"]);
    accepts("row-gap", &["normal", "0", "10px", "25%"]);
    rejects("column-gap", &["-1px"]);
    rejects("row-gap", &["-1px"]);
    accepts("z-index", &["auto", "-2", "0", "3"]);
    rejects("z-index", &["1.5"]);
    accepts("perspective", &["none", "10px"]);
    rejects("perspective", &["auto", "-1px"]);
}

#[test]
fn length_and_angle_units_are_the_documented_subset() {
    accepts("width", &["1px", "1rpx", "1em", "1rem", "1vw", "1vh", "0"]);
    rejects(
        "width",
        &["1ppx", "1sp", "1cm", "1pt", "1ex", "1vmin", "1dvw"],
    );
    accepts(
        "transform",
        &["rotate(1deg)", "rotate(1rad)", "rotate(1turn)"],
    );
    rejects("transform", &["rotate(1grad)"]);
}

#[test]
fn typography_grammars() {
    accepts(
        "font-size",
        &["14px", "1.25em", "120%", "medium", "smaller", "larger"],
    );
    accepts(
        "font-style",
        &["normal", "italic", "oblique", "oblique 10deg"],
    );
    accepts(
        "font-weight",
        &[
            "normal", "bold", "bolder", "lighter", "1", "100", "550", "1000", "100.5",
        ],
    );
    rejects("font-weight", &["0", "1001"]);
    accepts(
        "line-height",
        &["normal", "1.2", "20px", "120%", "calc(1 + 1)"],
    );
    accepts("letter-spacing", &["0", "1px", "0.1em"]);
    rejects("letter-spacing", &["normal", "10%"]);
}

#[test]
fn alignment_and_text_keyword_grammars() {
    accepts(
        "align-content",
        &[
            "stretch",
            "start",
            "end",
            "flex-start",
            "flex-end",
            "center",
            "space-between",
            "space-around",
        ],
    );
    rejects("align-content", &["normal", "baseline", "space-evenly"]);
    accepts("justify-content", &["stretch", "center", "space-evenly"]);
    rejects("justify-content", &["normal", "left", "safe center"]);
    accepts(
        "align-self",
        &[
            "auto",
            "stretch",
            "center",
            "start",
            "end",
            "flex-start",
            "flex-end",
            "baseline",
        ],
    );
    rejects("align-self", &["normal", "self-start", "safe center"]);
    accepts(
        "justify-self",
        &["auto", "stretch", "center", "start", "end"],
    );
    rejects(
        "justify-self",
        &["normal", "flex-start", "baseline", "safe center"],
    );
    accepts("align-items", &["stretch", "baseline", "flex-end"]);
    rejects(
        "align-items",
        &["auto", "normal", "self-start", "safe center"],
    );
    accepts("justify-items", &["stretch", "center", "start", "end"]);
    rejects(
        "justify-items",
        &["normal", "auto", "baseline", "flex-start"],
    );
    accepts("place-self", &["center", "flex-start center"]);
    rejects("place-self", &["flex-start", "baseline"]);
    accepts("place-items", &["center", "baseline center"]);
    rejects("place-items", &["baseline", "flex-start"]);

    accepts("text-align", &["start", "end", "left", "right", "center"]);
    rejects("text-align", &["justify", "match-parent", "-webkit-center"]);
    accepts("text-overflow", &["clip", "ellipsis"]);
    rejects("text-overflow", &["clip ellipsis", "\"more\""]);
    accepts(
        "text-decoration-line",
        &["none", "underline", "line-through"],
    );
    rejects(
        "text-decoration-line",
        &["overline", "blink", "underline line-through"],
    );
}

#[test]
fn colors_images_and_gradients() {
    accepts(
        "color",
        &[
            "red",
            "#1234",
            "rgb(1, 2, 3)",
            "hsl(120 100% 50%)",
            "linear-gradient(red, blue)",
            "radial-gradient(red 0%, blue 100%)",
            "conic-gradient(red 0, blue 1)",
        ],
    );
    rejects(
        "color",
        &[
            "currentcolor",
            "hwb(0 0% 0%)",
            "lab(50% 0 0)",
            "color-mix(in srgb, red, blue)",
            "repeating-linear-gradient(red, blue)",
            "linear-gradient(red)",
            "linear-gradient(red 10px, blue 20px)",
            "linear-gradient(red 0% 20%, blue)",
        ],
    );

    accepts(
        "background-image",
        &["none", "url(\"image.png\")", "linear-gradient(red, blue)"],
    );
    rejects(
        "background-image",
        &[
            "image-set(url(\"a.png\") 1x)",
            "cross-fade(url(\"a.png\"), url(\"b.png\"), 50%)",
            "repeating-radial-gradient(red, blue)",
        ],
    );
    accepts(
        "background-clip",
        &["border-box", "padding-box", "content-box", "border-area"],
    );
}

#[test]
fn supported_shorthands_keep_their_complete_standard_grammar() {
    accepts(
        "background-position",
        &["left top", "right 10px bottom 20px"],
    );
    accepts("white-space", &["pre-wrap", "preserve nowrap"]);
    accepts("border-image", &["url(\"border.png\") 30 / 10px / 2 round"]);
    accepts("font", &["italic 700 14px/20px serif"]);
    accepts("place-items", &["center stretch"]);
}

#[test]
fn effects_and_motion_grammars() {
    accepts(
        "filter",
        &[
            "none",
            "blur(2px)",
            "brightness(120%)",
            "contrast(1.2) grayscale(30%)",
            "saturate(2)",
        ],
    );
    rejects(
        "filter",
        &[
            "sepia(1)",
            "invert(1)",
            "hue-rotate(10deg)",
            "drop-shadow(1px 1px red)",
        ],
    );
    accepts(
        "box-shadow",
        &[
            "1px 2px 3px red",
            "1px 2px",
            "inset 1px 2px red",
            "1px 2px red, inset 3px 4px 5px blue",
        ],
    );
    accepts("text-shadow", &["1px 2px 3px red"]);
    rejects(
        "text-shadow",
        &[
            "red 1px 2px 3px",
            "1px 2px red",
            "1px 2px 3px red, 0 0 1px blue",
        ],
    );

    accepts(
        "clip-path",
        &[
            "none",
            "inset(1px)",
            "circle(50%)",
            "ellipse(40% 30%)",
            "path(\"M 0 0 L 1 1\")",
        ],
    );
    rejects("clip-path", &["polygon(0 0, 100% 100%)", "url(#clip)"]);
    accepts(
        "offset-path",
        &["inset(1px)", "circle(50%)", "path(\"M 0 0 L 1 1\")"],
    );
    rejects(
        "offset-path",
        &[
            "none",
            "ray(45deg)",
            "url(#path)",
            "polygon(0 0, 100% 100%)",
        ],
    );
    accepts("offset-rotate", &["auto", "0deg", "180deg", "1turn"]);
    rejects(
        "offset-rotate",
        &["reverse", "auto 10deg", "-1deg", "361deg"],
    );
}

#[test]
fn transform_cursor_and_timing_grammars() {
    accepts(
        "transform",
        &[
            "none",
            "translate(10px, 20px)",
            "scale(1.5)",
            "rotate(45deg)",
            "skew(10deg, 20deg)",
        ],
    );
    rejects(
        "transform",
        &[
            "scale(50%)",
            "scaleZ(2)",
            "scale3d(1, 2, 3)",
            "rotate3d(1, 0, 0, 45deg)",
            "perspective(100px)",
        ],
    );
    accepts("transform-origin", &["center", "left top", "25% 75%"]);
    rejects("transform-origin", &["left top 10px"]);
    accepts("cursor", &["auto", "pointer", "grab", "zoom-in"]);
    rejects("cursor", &["url(\"cursor.png\"), pointer", "-moz-grab"]);

    accepts(
        "animation-timing-function",
        &[
            "linear",
            "ease",
            "steps(2, end)",
            "cubic-bezier(0, 0, 1, 1)",
            "square-bezier(0.5, 0.5)",
        ],
    );
    rejects("animation-timing-function", &["linear(0, 1)"]);
    accepts("animation-duration", &["0s", "250ms"]);
    rejects("animation-duration", &["auto", "100"]);
}

#[test]
fn grid_and_lynx_layout_grammars() {
    accepts(
        "grid-column-start",
        &["auto", "1", "-2", "header", "span 2", "span header"],
    );
    rejects("grid-column-start", &["0", "span -1"]);
    accepts("grid-column-end", &["span 2", "span header"]);
    accepts(
        "grid-template-columns",
        &[
            "none",
            "10px 1fr",
            "min-content",
            "minmax(10px, 1fr)",
            "[start] 10px",
            "repeat(3, 20px)",
            "repeat(2, 10px 20px)",
            "repeat(auto-fit, 20px)",
        ],
    );
    rejects("grid-template-columns", &["repeat(2)"]);

    accepts(
        "linear-direction",
        &["row", "row-reverse", "column", "column-reverse"],
    );
    rejects("linear-direction", &["horizontal", "vertical", "normal"]);
    accepts("linear-weight", &["0", "1.5"]);
    rejects("linear-weight", &["-1"]);
    accepts("relative-id", &["1", "10"]);
    rejects("relative-id", &["-1", "0", "none"]);
    accepts("relative-align-top", &["none", "parent", "1"]);
    rejects("relative-align-top", &["0", "-1"]);
    accepts("relative-top-of", &["none", "1"]);
    rejects("relative-top-of", &["parent", "0", "-1"]);
    accepts(
        "relative-center",
        &["none", "vertical", "horizontal", "both"],
    );
    accepts("relative-layout-once", &["true", "false"]);
}

#[test]
fn w3c_will_change_grammar() {
    accepts(
        "contain",
        &[
            "none",
            "strict",
            "content",
            "size",
            "inline-size",
            "layout",
            "style",
            "paint",
            "layout paint",
        ],
    );
    rejects("contain", &["none layout", "strict paint", "content size"]);
    accepts(
        "will-change",
        &[
            "auto",
            "transform",
            "opacity",
            "contain",
            "scroll-position",
            "contents",
            "transform, opacity",
        ],
    );
    rejects("will-change", &["none", "all", "will-change"]);
}
