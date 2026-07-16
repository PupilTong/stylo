/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Computed color values.

use crate::color::AbsoluteColor;
#[cfg(feature = "lynx")]
use crate::derives::*;
use crate::typed_om::{KeywordValue, ToTyped, TypedValue};
use crate::values::animated::ToAnimatedZero;
use crate::values::computed::percentage::Percentage;
use crate::values::generics::color::{
    GenericCaretColor, GenericColor, GenericColorMix, GenericColorOrAuto,
};
use std::fmt::{self, Write};
use style_traits::{CssString, CssWriter, ToCss};
use thin_vec::ThinVec;

pub use crate::values::specified::color::{ColorScheme, ForcedColorAdjust, PrintColorAdjust};

/// The computed value of the standard `color` property.
#[cfg(not(feature = "lynx"))]
pub type ColorPropertyValue = AbsoluteColor;

/// The computed value of Lynx's `color` property.
///
/// Lynx extends the property's grammar from `<color>` to
/// `<color> | <gradient>`, so its computed representation must preserve the
/// gradient for the text painter rather than collapsing it to a fallback
/// solid color.
#[cfg(feature = "lynx")]
#[derive(Clone, Debug, MallocSizeOf, PartialEq, ToResolvedValue)]
pub enum ColorPropertyValue {
    /// A solid text color.
    Color(AbsoluteColor),
    /// A text gradient.
    Gradient(Box<crate::values::computed::image::Gradient>),
}

#[cfg(feature = "lynx")]
impl ColorPropertyValue {
    /// Return the solid color used by Stylo internals that resolve
    /// `currentcolor`. Lynx does not accept authored `currentcolor`; a
    /// gradient therefore has no solid current-color and uses transparent as
    /// the compatibility fallback. The actual CSS initial value remains
    /// Stylo's standard black; Lynx defaults belong in the UA stylesheet.
    #[inline]
    pub fn solid_color(&self) -> AbsoluteColor {
        match *self {
            Self::Color(color) => color,
            Self::Gradient(..) => AbsoluteColor::TRANSPARENT_BLACK,
        }
    }
}

#[cfg(feature = "lynx")]
impl From<AbsoluteColor> for ColorPropertyValue {
    fn from(color: AbsoluteColor) -> Self {
        Self::Color(color)
    }
}

#[cfg(feature = "lynx")]
impl ToCss for ColorPropertyValue {
    fn to_css<W>(&self, dest: &mut CssWriter<W>) -> fmt::Result
    where
        W: fmt::Write,
    {
        match *self {
            Self::Color(ref color) => color.to_css(dest),
            Self::Gradient(ref gradient) => gradient.to_css(dest),
        }
    }
}

#[cfg(feature = "lynx")]
impl ToTyped for ColorPropertyValue {
    fn to_typed(&self, dest: &mut ThinVec<TypedValue>) -> Result<(), ()> {
        match *self {
            Self::Color(ref color) => color.to_typed(dest),
            Self::Gradient(..) => Err(()),
        }
    }
}

/// A computed value for `<color>`.
pub type Color = GenericColor<Percentage>;

/// A computed color-mix().
pub type ColorMix = GenericColorMix<Color, Percentage>;

impl ToCss for Color {
    fn to_css<W>(&self, dest: &mut CssWriter<W>) -> fmt::Result
    where
        W: fmt::Write,
    {
        match *self {
            Self::Absolute(ref c) => c.to_css(dest),
            Self::ColorFunction(ref color_function) => color_function.to_css(dest),
            Self::CurrentColor => dest.write_str("currentcolor"),
            Self::ColorMix(ref m) => m.to_css(dest),
            Self::ContrastColor(ref c) => {
                dest.write_str("contrast-color(")?;
                c.to_css(dest)?;
                dest.write_char(')')
            },
        }
    }
}

impl ToTyped for Color {
    fn to_typed(&self, dest: &mut ThinVec<TypedValue>) -> Result<(), ()> {
        match *self {
            Self::CurrentColor => {
                dest.push(TypedValue::Keyword(KeywordValue(CssString::from(
                    "currentcolor",
                ))));
                Ok(())
            },
            _ => Err(()),
        }
    }
}

impl Color {
    /// A fully transparent color.
    pub const TRANSPARENT_BLACK: Self = Self::Absolute(AbsoluteColor::TRANSPARENT_BLACK);

    /// An opaque black color.
    pub const BLACK: Self = Self::Absolute(AbsoluteColor::BLACK);

    /// An opaque white color.
    pub const WHITE: Self = Self::Absolute(AbsoluteColor::WHITE);

    /// Create a new computed [`Color`] from a given color-mix, simplifying it to an absolute color
    /// if possible.
    pub fn from_color_mix(color_mix: ColorMix) -> Self {
        if let Some(absolute) = color_mix.mix_to_absolute() {
            Self::Absolute(absolute)
        } else {
            Self::ColorMix(Box::new(color_mix))
        }
    }

    /// Combine this complex color with the given foreground color into an absolute color.
    pub fn resolve_to_absolute(&self, current_color: &AbsoluteColor) -> AbsoluteColor {
        match *self {
            Self::Absolute(c) => c,
            Self::ColorFunction(ref color_function) => {
                color_function.resolve_to_absolute(current_color)
            },
            Self::CurrentColor => *current_color,
            Self::ColorMix(ref mix) => {
                use crate::color::mix;

                mix::mix_many(
                    mix.interpolation,
                    mix.items.iter().map(|item| {
                        mix::ColorMixItem::new(
                            item.color.resolve_to_absolute(current_color),
                            item.percentage.0,
                        )
                    }),
                    mix.flags,
                )
            },
            Self::ContrastColor(ref c) => {
                let bg_color = c.resolve_to_absolute(current_color);
                if Self::contrast_ratio(&bg_color, &AbsoluteColor::BLACK)
                    > Self::contrast_ratio(&bg_color, &AbsoluteColor::WHITE)
                {
                    AbsoluteColor::BLACK
                } else {
                    AbsoluteColor::WHITE
                }
            },
        }
    }

    fn contrast_ratio(a: &AbsoluteColor, b: &AbsoluteColor) -> f32 {
        // TODO: This just implements the WCAG 2.1 algorithm,
        // https://www.w3.org/TR/WCAG21/#dfn-contrast-ratio
        // Consider using a more sophisticated contrast algorithm, e.g. see
        // https://apcacontrast.com
        let compute = |c| -> f32 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                f32::powf((c + 0.055) / 1.055, 2.4)
            }
        };
        let luminance = |r, g, b| -> f32 { 0.2126 * r + 0.7152 * g + 0.0722 * b };
        let a = a.into_srgb_legacy();
        let b = b.into_srgb_legacy();
        let a = a.raw_components();
        let b = b.raw_components();
        let la = luminance(compute(a[0]), compute(a[1]), compute(a[2])) + 0.05;
        let lb = luminance(compute(b[0]), compute(b[1]), compute(b[2])) + 0.05;
        if la > lb {
            la / lb
        } else {
            lb / la
        }
    }
}

impl ToAnimatedZero for AbsoluteColor {
    fn to_animated_zero(&self) -> Result<Self, ()> {
        Ok(Self::TRANSPARENT_BLACK)
    }
}

/// auto | <color>
pub type ColorOrAuto = GenericColorOrAuto<Color>;

/// caret-color
pub type CaretColor = GenericCaretColor<Color>;
