// Verifies that the CSS properties LynxJS supports still parse from content
// under the `lynx` feature (i.e. the allowlist in properties/data.py did not
// disable anything Lynx exposes). Gated behind `lynx`; nothing to test without
// it.
#![cfg(feature = "lynx")]

use style::properties::PropertyId;

/// Whether `name` parses as a content-enabled property (the gate every author
/// stylesheet / CSSOM `setProperty` funnels through).
fn is_content_enabled(name: &str) -> bool {
    PropertyId::parse_enabled_for_all_content(name).is_ok()
}

/// A representative slice of the Lynx-supported set: box model, flex, grid,
/// backgrounds/borders (incl. the inline-logical longhands), text, effects and
/// the Lynx-only linear-*/relative-* additions.
const LYNX_SUPPORTED: &[&str] = &[
    // Box / positioning / sizing.
    "display",
    "position",
    "top",
    "right",
    "bottom",
    "left",
    "inset-inline-start",
    "inset-inline-end",
    "width",
    "height",
    "min-width",
    "max-width",
    "min-height",
    "max-height",
    "box-sizing",
    "box-shadow",
    "overflow",
    "overflow-x",
    "overflow-y",
    "z-index",
    "opacity",
    "visibility",
    "pointer-events",
    "aspect-ratio",
    // Flex + grid + alignment.
    "flex",
    "flex-basis",
    "flex-direction",
    "flex-grow",
    "flex-shrink",
    "flex-wrap",
    "flex-flow",
    "order",
    "align-items",
    "align-self",
    "align-content",
    "justify-content",
    "justify-items",
    "justify-self",
    "gap",
    "row-gap",
    "column-gap",
    // Margins / paddings (physical + inline-logical).
    "margin",
    "margin-top",
    "margin-left",
    "margin-inline-start",
    "margin-inline-end",
    "padding",
    "padding-bottom",
    "padding-inline-start",
    "padding-inline-end",
    // Color / backgrounds / borders.
    "color",
    "background",
    "background-color",
    "background-image",
    "background-position",
    "border",
    "border-top-color",
    "border-radius",
    "border-inline-start-color",
    "border-inline-end-width",
    "border-start-start-radius",
    "border-end-end-radius",
    // Text.
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "line-height",
    "letter-spacing",
    "text-align",
    "text-decoration",
    "text-indent",
    "text-shadow",
    "word-break",
    "white-space",
    "direction",
    "cursor",
    // Unprefixed Lynx spellings that alias stylo's gecko-only -webkit-text-stroke*.
    "text-stroke",
    "text-stroke-color",
    "text-stroke-width",
    // Effects / transforms / motion.
    "transform",
    "transform-origin",
    "perspective",
    "filter",
    "clip-path",
    "image-rendering",
    // Grid / mask / motion / text-overflow — Lynx-supported; the `lynx` feature
    // drops the servo pref (`layout.grid.enabled`, `layout.unimplemented`) that
    // keeps these experimental in a stock stylo build, so they parse from content.
    "grid-template-columns",
    "grid-template-rows",
    "grid-auto-flow",
    "grid-auto-columns",
    "grid-auto-rows",
    "grid-column",
    "grid-row",
    "mask",
    "mask-image",
    "mask-composite",
    "offset-path",
    "offset-distance",
    "offset-rotate",
    "text-overflow",
    // Animation / transition.
    "animation",
    "animation-name",
    "animation-duration",
    "transition",
    "transition-property",
    "transition-duration",
    // Lynx-only additions.
    "linear-direction",
    "linear-weight",
    "linear-weight-sum",
    "relative-id",
    "relative-center",
    "relative-layout-once",
    "relative-align-top",
    "relative-align-inline-start",
    "relative-top-of",
    "relative-inline-start-of",
];

#[test]
fn lynx_supported_properties_are_content_enabled() {
    for &name in LYNX_SUPPORTED {
        assert!(
            is_content_enabled(name),
            "`{name}` is Lynx-supported and must stay content-enabled under the `lynx` feature",
        );
    }
}

#[test]
fn custom_properties_are_still_supported() {
    // Custom properties go through a different id space and must be unaffected.
    assert!(is_content_enabled("--lynx-custom"));
}
