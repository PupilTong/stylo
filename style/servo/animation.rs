/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! CSS transitions and animations.

// NOTE(emilio): This code isn't really executed in Gecko, but we don't want to
// compile it out so that people remember it exists.

use crate::Atom;
use crate::FxHashMap;
use crate::context::{CascadeInputs, SharedStyleContext};
use crate::derives::*;
use crate::dom::{OpaqueNode, TDocument, TElement, TNode};
use crate::properties::AnimationDeclarations;
use crate::properties::animated_properties::{AnimationValue, AnimationValueMap};
use crate::properties::longhands::animation_composition::computed_value::single_value::T as AnimationComposition;
use crate::properties::longhands::animation_direction::computed_value::single_value::T as AnimationDirection;
use crate::properties::longhands::animation_fill_mode::computed_value::single_value::T as AnimationFillMode;
use crate::properties::longhands::animation_play_state::computed_value::single_value::T as AnimationPlayState;
use crate::properties::{
    ComputedValues, Importance, LonghandId, PropertyDeclarationBlock, PropertyDeclarationId,
    PropertyDeclarationIdSet,
};
use crate::rule_tree::{CascadeLevel, CascadeOrigin, RuleCascadeFlags};
use crate::selector_parser::PseudoElement;
use crate::shared_lock::{Locked, SharedRwLock};
use crate::style_resolver::StyleResolverForElement;
use crate::stylesheets::keyframes_rule::{
    KeyframeSelector, KeyframesAnimation, KeyframesStep, KeyframesStepValue,
};
use crate::stylesheets::layer_rule::LayerOrder;
use crate::values::animated::{Animate, Procedure};
use crate::values::computed::{AnimationTimeline, TimingFunction};
use crate::values::generics::easing::BeforeFlag;
use crate::values::specified::animation::TimelineRangeName;
use crate::values::specified::TransitionBehavior;
use crate::ArcSlice;
use parking_lot::RwLock;
use servo_arc::Arc;
use std::fmt;

/// Represents an animation for a given property.
#[derive(Clone, Debug, MallocSizeOf)]
pub struct PropertyAnimation {
    /// The value we are animating from.
    from: AnimationValue,

    /// The value we are animating to.
    to: AnimationValue,

    /// The timing function of this `PropertyAnimation`.
    timing_function: TimingFunction,

    /// The duration of this `PropertyAnimation` in seconds.
    pub duration: f64,
}

impl PropertyAnimation {
    /// Returns the given property longhand id.
    pub fn property_id(&self) -> PropertyDeclarationId<'_> {
        debug_assert_eq!(self.from.id(), self.to.id());
        self.from.id()
    }

    /// The value this animation starts from.
    pub fn from(&self) -> &AnimationValue {
        &self.from
    }

    /// The value this animation ends at.
    pub fn to(&self) -> &AnimationValue {
        &self.to
    }

    /// The timing function applied to this animation's progress.
    pub fn timing_function(&self) -> &TimingFunction {
        &self.timing_function
    }

    /// The output of the timing function given the progress ration of this animation.
    fn timing_function_output(&self, progress: f64) -> f64 {
        eased(&self.timing_function, self.duration, progress)
    }

    /// Update the given animation at a given point of progress.
    fn calculate_value(&self, progress: f64) -> AnimationValue {
        interpolate(
            &self.from,
            &self.to,
            &self.timing_function,
            self.duration,
            progress,
        )
    }
}

/// The output of `timing_function` at `progress` through an interval
/// `duration` long, which sets the precision of its solution.
fn eased(timing_function: &TimingFunction, duration: f64, progress: f64) -> f64 {
    let epsilon = 1. / (200. * duration);
    // FIXME: Need to set the before flag correctly.
    // In order to get the before flag, we have to know the current animation phase
    // and whether the iteration is reversed. For now, we skip this calculation
    // by treating as if the flag is unset at all times.
    // https://drafts.csswg.org/css-easing/#step-timing-function-algo
    timing_function.calculate_output(progress, BeforeFlag::Unset, epsilon)
}

/// The value `progress` through the interpolation from `from` to `to`, eased
/// by `timing_function` over an interval `duration` long.
fn interpolate(
    from: &AnimationValue,
    to: &AnimationValue,
    timing_function: &TimingFunction,
    duration: f64,
    progress: f64,
) -> AnimationValue {
    let progress = eased(timing_function, duration, progress);
    let procedure = Procedure::Interpolate { progress };
    from.animate(to, procedure).unwrap_or_else(|()| {
        // Fall back to discrete interpolation
        if progress < 0.5 {
            from.clone()
        } else {
            to.clone()
        }
    })
}

/// This structure represents the state of an animation.
#[derive(Clone, Debug, MallocSizeOf, PartialEq)]
pub enum AnimationState {
    /// The animation has been created, but is not running yet. This state
    /// is also used when an animation is still in the first delay phase.
    Pending,
    /// This animation is currently running.
    Running,
    /// This animation is paused. The inner field is the percentage of progress
    /// when it was paused, from 0 to 1.
    Paused(f64),
    /// This animation has finished.
    Finished,
    /// This animation has been canceled.
    Canceled,
}

impl AnimationState {
    /// Whether or not this state requires its owning animation to be ticked.
    fn needs_to_be_ticked(&self) -> bool {
        *self == AnimationState::Running || *self == AnimationState::Pending
    }
}

enum IgnoreTransitions {
    Canceled,
    CanceledAndFinished,
}

/// This structure represents a keyframes animation current iteration state.
///
/// If the iteration count is infinite, there's no other state, otherwise we
/// have to keep track the current iteration and the max iteration count.
#[derive(Clone, Debug, MallocSizeOf)]
pub enum KeyframesIterationState {
    /// Infinite iterations with the current iteration count.
    Infinite(f64),
    /// Current and max iterations.
    Finite(f64, f64),
}

/// The fields iterating an animation moves, apart from the animation, so a
/// sampler can advance a copy of them.
#[derive(Clone, Debug)]
struct IterationCursor {
    started_at: f64,
    iteration_state: KeyframesIterationState,
    current_direction: AnimationDirection,
}

impl IterationCursor {
    /// See [`Animation::iterate_by`].
    fn iterate_by(&mut self, n: f64, duration: f64, direction: AnimationDirection) -> f64 {
        let n = n.trunc().min(self.remaining_iterations().ceil() - 1.0);
        if n < 1. {
            return 0.;
        }

        match self.iteration_state {
            KeyframesIterationState::Finite(ref mut current, max) => {
                *current = (*current + n).min(max);
            },
            KeyframesIterationState::Infinite(ref mut current) => {
                *current += n;
            },
        }

        // Update the next iteration direction if applicable.
        self.started_at += duration * n;
        match direction {
            AnimationDirection::Alternate | AnimationDirection::AlternateReverse
                if n % 2. == 1.0 =>
            {
                self.current_direction = match self.current_direction {
                    AnimationDirection::Normal => AnimationDirection::Reverse,
                    AnimationDirection::Reverse => AnimationDirection::Normal,
                    _ => unreachable!(
                        "Current animation direction can only be `normal` or `reverse`."
                    ),
                };
            },
            _ => {},
        }

        n
    }

    /// See [`Animation::iterate_to`].
    fn iterate_to(&mut self, now: f64, duration: f64, direction: AnimationDirection) -> bool {
        // Below this every integer is exact, so `whole` counts iterations.
        const EXACT: f64 = (1u64 << 52) as f64;
        // The single steps after the jump: one or two, one more after a jump
        // one iteration shorter, and one for rounding.
        const TAIL: usize = 4;

        if !self.iteration_over(now, duration) {
            return false;
        }
        let mut iterated = false;
        // All but the last one or two whole iterations elapsed, in one step
        // that stays short of `now`: one iteration fewer when rounding
        // carries `whole` to or past it, none when that does too.
        let whole = ((now - self.started_at) / duration).ceil() - 2.;
        if whole >= 1. && whole < EXACT {
            let jump = [whole, whole - 1.]
                .into_iter()
                .find(|n| self.started_at + duration * n < now);
            if let Some(n) = jump {
                iterated = self.iterate_by(n, duration, direction) > 0.;
            }
        }
        // The rest one at a time, with the check `iterate_if_necessary`
        // makes; a step that leaves `started_at` unchanged ends the walk.
        for _ in 0..TAIL {
            let started_at = self.started_at;
            let over = self.iteration_over(now, duration);
            if !over || self.iterate_by(1., duration, direction) == 0. {
                break;
            }
            iterated = true;
            if self.started_at == started_at {
                break;
            }
        }
        iterated
    }

    fn remaining_iterations(&self) -> f64 {
        match self.iteration_state {
            KeyframesIterationState::Finite(current, max) => max - current,
            KeyframesIterationState::Infinite(_) => f64::INFINITY,
        }
    }

    fn current_iteration_end_progress(&self) -> f64 {
        self.remaining_iterations().min(1.)
    }

    fn iteration_over(&self, time: f64, duration: f64) -> bool {
        time > (self.started_at + self.current_iteration_end_progress() * duration)
    }

    /// Whether a running animation's current iteration has reached its end
    /// progress at `time`.
    fn iteration_ended(&self, time: f64, duration: f64) -> bool {
        (time - self.started_at) / duration >= self.current_iteration_end_progress()
    }

    fn on_last_iteration(&self) -> bool {
        self.remaining_iterations() <= 1.
    }
}

/// Where an animation stands in its current iteration: the simple iteration
/// progress (css-animations-1 / web-animations-1 §4.7, before the direction
/// is applied) and whether the iteration runs reversed.
/// [`Animation::sample_at`] applies the direction itself: a reversed
/// iteration reads the keyframe offset `1 - progress`. Never NaN.
#[derive(Clone, Copy, Debug, MallocSizeOf, PartialEq)]
pub struct AnimationProgress {
    /// In `[0, 1]`, or negative in a backwards-filled before phase.
    progress: f64,
    /// `normal` or `reverse`.
    direction: AnimationDirection,
}

impl AnimationProgress {
    /// A simple iteration progress through an iteration running forward, or
    /// reversed when `reversed`; `progress` is clamped to `[0, 1]`, NaN
    /// reading as 0.
    pub fn new(progress: f64, reversed: bool) -> Self {
        Self {
            // `f64::max` answers 0 for NaN.
            progress: progress.max(0.).min(1.),
            direction: Self::direction_of(reversed),
        }
    }

    /// A backwards-filled before phase: the negative progress the time path
    /// reads there. [`Animation::sample_at`] then samples offset 0 (1 when
    /// `reversed`), taking a keyframe there without easing it, as the before
    /// flag requires for `steps(jump-start)`.
    pub fn before_phase(reversed: bool) -> Self {
        Self {
            progress: -1.,
            direction: Self::direction_of(reversed),
        }
    }

    fn direction_of(reversed: bool) -> AnimationDirection {
        if reversed {
            AnimationDirection::Reverse
        } else {
            AnimationDirection::Normal
        }
    }

    /// The progress: in `[0, 1]`, or negative in a backwards-filled before
    /// phase.
    pub fn progress(&self) -> f64 {
        self.progress
    }

    /// The direction the iteration runs: `normal` or `reverse`.
    pub fn direction(&self) -> AnimationDirection {
        self.direction
    }
}

/// A temporary data structure used when calculating DeclaredKeyframes for an
/// animation. This data structure is used to collapse information for steps
/// which may be spread across multiple keyframe declarations into a single
/// instance per `start_percentage`.
#[derive(Debug)]
struct IntermediateComputedKeyframe {
    declarations: PropertyDeclarationBlock,
    timing_function: Option<TimingFunction>,
    composition: Option<AnimationComposition>,
    start_percentage: f64,
}

impl IntermediateComputedKeyframe {
    fn new(start_percentage: f64) -> Self {
        IntermediateComputedKeyframe {
            declarations: PropertyDeclarationBlock::new(),
            timing_function: None,
            composition: None,
            start_percentage,
        }
    }

    /// Walk through all keyframe declarations and combine all declarations with the
    /// same `start_percentage` into individual `IntermediateComputedKeyframe`s.
    fn generate_for_keyframes(
        animation: &KeyframesAnimation,
        context: &SharedStyleContext,
        base_style: &ComputedValues,
    ) -> Vec<Self> {
        if animation.steps.is_empty() {
            return vec![];
        }

        let mut intermediate_steps: Vec<Self> = Vec::with_capacity(animation.steps.len());
        let mut current_step = IntermediateComputedKeyframe::new(0.);
        for step in animation.steps.iter() {
            let start_percentage = step.start_offset.percentage.0 as f64;
            if start_percentage != current_step.start_percentage {
                let new_step = IntermediateComputedKeyframe::new(start_percentage);
                intermediate_steps.push(std::mem::replace(&mut current_step, new_step));
            }

            current_step.update_from_step(step, context, base_style);
        }
        intermediate_steps.push(current_step);

        // We should always have a first and a last step, even if these are just
        // generated by KeyframesStepValue::ComputedValues.
        debug_assert!(intermediate_steps.first().unwrap().start_percentage == 0.);
        debug_assert!(intermediate_steps.last().unwrap().start_percentage == 1.);

        intermediate_steps
    }

    /// Walk through the keyframes attached to a named timeline range in
    /// specified order, and collapse those with the same selector into the
    /// earliest of them (csswg-drafts#8507), each with its range name; the
    /// `start_percentage` is the selector's percentage, of any value.
    fn generate_for_range_keyframes(
        animation: &KeyframesAnimation,
        context: &SharedStyleContext,
        base_style: &ComputedValues,
    ) -> Vec<(TimelineRangeName, Self)> {
        let mut steps: Vec<(TimelineRangeName, Self)> = Vec::new();
        for step in animation.steps_with_range_name.iter() {
            let KeyframeSelector {
                range_name,
                percentage,
            } = step.start_offset;
            let percentage = percentage.0 as f64;
            let index = steps
                .iter()
                .position(|(name, merged)| {
                    *name == range_name && merged.start_percentage == percentage
                })
                .unwrap_or_else(|| {
                    steps.push((range_name, IntermediateComputedKeyframe::new(percentage)));
                    steps.len() - 1
                });
            steps[index].1.update_from_step(step, context, base_style);
        }
        steps
    }

    fn update_from_step(
        &mut self,
        step: &KeyframesStep,
        context: &SharedStyleContext,
        base_style: &ComputedValues,
    ) {
        // Each keyframe declaration may optionally specify a timing function, falling
        // back to the one defined global for the animation.
        let guard = &context.guards.author;
        if let Some(timing_function) = step.get_animation_timing_function(&guard) {
            self.timing_function = Some(timing_function.to_computed_value_without_context());
        }

        // Each keyframe declaration may optionally specify a composite operation,
        // falling back to the one defined globally for the animation.
        if let Some(composition) = step.get_animation_composition(&guard) {
            self.composition = Some(composition);
        }

        let block = match step.value {
            KeyframesStepValue::ComputedValues => return,
            KeyframesStepValue::Declarations { ref block } => block,
        };

        // Filter out !important, non-animatable properties, and the
        // 'display' property (which is only animatable from SMIL).
        let guard = block.read_with(&guard);
        for declaration in guard.normal_declaration_iter() {
            if let PropertyDeclarationId::Longhand(id) = declaration.id() {
                if id == LonghandId::Display {
                    continue;
                }

                if !id.is_animatable() {
                    continue;
                }
            }

            self.declarations.push(
                declaration.to_physical(base_style.writing_mode),
                Importance::Normal,
            );
        }
    }

    fn resolve_style<E>(
        self,
        element: E,
        context: &SharedStyleContext,
        base_style: &Arc<ComputedValues>,
        resolver: &mut StyleResolverForElement<E>,
    ) -> Arc<ComputedValues>
    where
        E: TElement,
    {
        if !self.declarations.any_normal() {
            return base_style.clone();
        }

        let document = element.as_node().owner_doc();
        let locked_block = Arc::new(document.shared_lock().wrap(self.declarations));
        let mut important_rules_changed = false;
        let rule_node = base_style.rules().clone();
        let new_node = context.stylist.rule_tree().update_rule_at_level(
            CascadeLevel::new(CascadeOrigin::Animations),
            LayerOrder::root(),
            Some(locked_block.borrow_arc()),
            &rule_node,
            &context.guards,
            &mut important_rules_changed,
        );

        if new_node.is_none() {
            return base_style.clone();
        }

        let inputs = CascadeInputs {
            rules: new_node,
            visited_rules: base_style.visited_rules().cloned(),
            flags: base_style.flags.for_cascade_inputs(),
            included_cascade_flags: RuleCascadeFlags::empty(),
        };
        resolver
            .cascade_style_and_visited_with_default_parents(inputs)
            .0
    }
}

/// The named timeline ranges a range keyframe selector names, in the order
/// [`TimelineRanges`] stores them.
const NAMED_RANGES: [TimelineRangeName; 7] = [
    TimelineRangeName::Cover,
    TimelineRangeName::Contain,
    TimelineRangeName::Entry,
    TimelineRangeName::Exit,
    TimelineRangeName::EntryCrossing,
    TimelineRangeName::ExitCrossing,
    TimelineRangeName::Scroll,
];

/// The named timeline ranges of an animation's timeline (scroll-animations-1
/// §3), each as the fractions of the animation's attachment range its start
/// and end sit at: 0 at the attachment range's start, 1 at its end. They
/// place the animation's range keyframes (§5). Every bound is finite.
#[derive(Clone, Copy, Debug, MallocSizeOf, PartialEq)]
pub struct TimelineRanges(#[ignore_malloc_size_of = "Holds no heap data"] [[f64; 2]; 7]);

impl TimelineRanges {
    /// The ranges `range` answers for each named range, or `None` when it
    /// answers none for one or a bound is not finite.
    pub fn from_fn(mut range: impl FnMut(TimelineRangeName) -> Option<[f64; 2]>) -> Option<Self> {
        let mut ranges = [[0.; 2]; 7];
        for (slot, name) in ranges.iter_mut().zip(NAMED_RANGES) {
            *slot = range(name).filter(|bounds| bounds.iter().all(|bound| bound.is_finite()))?;
        }
        Some(Self(ranges))
    }

    /// The keyframe offset `percentage` (a fraction, of any value) of the
    /// range `name` sits at, exact at both ends of the range; `None` for a
    /// name that is no named range, or an offset that is not finite.
    fn offset(&self, name: TimelineRangeName, percentage: f64) -> Option<f64> {
        let [start, end] = self.0[NAMED_RANGES.iter().position(|named| *named == name)?];
        let offset = (1. - percentage) * start + percentage * end;
        offset.is_finite().then_some(offset)
    }
}

/// Where a keyframe selector attaches its keyframe.
#[derive(Clone, Copy, Debug, MallocSizeOf)]
enum KeyframePosition {
    /// A percentage, `from` and `to` included: the keyframe offset, in
    /// `[0, 1]`.
    Offset(f64),
    /// `<timeline-range-name> <percentage>`, the percentage as a fraction of
    /// any value: placed by the animation's [`TimelineRanges`], and ignored
    /// without them.
    Range(TimelineRangeName, f64),
}

impl KeyframePosition {
    /// The keyframe offset, or `None` when the keyframe is ignored.
    fn resolve(&self, ranges: Option<&TimelineRanges>) -> Option<f64> {
        match *self {
            Self::Offset(offset) => Some(offset),
            Self::Range(name, percentage) => ranges?.offset(name, percentage),
        }
    }
}

/// One keyframe of an `@keyframes` rule, merged with the keyframes it
/// collapses with: the percentage keyframes of one percentage, or the range
/// keyframes of one selector.
#[derive(Clone, Debug, MallocSizeOf)]
struct DeclaredKeyframe {
    position: KeyframePosition,
    /// The timing function of the segments this keyframe starts.
    timing_function: TimingFunction,
    /// For each animating property, the value this keyframe declares,
    /// composited onto the base value, or `None` where it declares none.
    values: Box<[Option<AnimationValue>]>,
}

/// An animation's keyframes as its `@keyframes` rule declares them, computed
/// once against the element's style: what [`DeclaredKeyframes::tracks`]
/// places for a timeline.
#[derive(Clone, Debug, MallocSizeOf)]
struct DeclaredKeyframes {
    /// The percentage keyframes ascending, then the range keyframes in
    /// specified order: css-animations-2's computed keyframe order, which
    /// breaks ties between equal offsets. Each declares an animating
    /// property.
    keyframes: Box<[DeclaredKeyframe]>,
    /// The base value of each animating property: an automatic keyframe's
    /// value.
    base: Box<[AnimationValue]>,
    /// The timing functions of the automatic 0% and 100% keyframes: those of
    /// the rule's 0% and 100% keyframes, else the animation's.
    automatic_timing: [TimingFunction; 2],
}

impl DeclaredKeyframes {
    fn new<E>(
        element: E,
        animation: &KeyframesAnimation,
        context: &SharedStyleContext,
        base_style: &Arc<ComputedValues>,
        default_timing_function: TimingFunction,
        default_composition: AnimationComposition,
        resolver: &mut StyleResolverForElement<E>,
        animating_properties: &PropertyDeclarationIdSet,
    ) -> Self
    where
        E: TElement,
    {
        let base: Box<[AnimationValue]> = animating_properties
            .iter()
            .map(|property| {
                AnimationValue::from_computed_values(property, &**base_style)
                    .expect("Unexpected non-animatable property.")
            })
            .collect();

        let percentage_steps =
            IntermediateComputedKeyframe::generate_for_keyframes(animation, context, base_style);
        let range_steps = IntermediateComputedKeyframe::generate_for_range_keyframes(
            animation, context, base_style,
        );
        let timing_of = |step: Option<&IntermediateComputedKeyframe>| {
            step.and_then(|step| step.timing_function.clone())
                .unwrap_or_else(|| default_timing_function.clone())
        };
        let automatic_timing = [
            timing_of(percentage_steps.first()),
            timing_of(percentage_steps.last()),
        ];

        let steps =
            percentage_steps
                .into_iter()
                .map(|step| (KeyframePosition::Offset(step.start_percentage), step))
                .chain(range_steps.into_iter().map(|(name, step)| {
                    (KeyframePosition::Range(name, step.start_percentage), step)
                }));
        let mut keyframes = Vec::new();
        for (position, step) in steps {
            let declared = step.declarations.property_ids().clone();
            if !animating_properties
                .iter()
                .any(|property| declared.contains(property))
            {
                continue;
            }
            let timing_function = step
                .timing_function
                .clone()
                .unwrap_or_else(|| default_timing_function.clone());
            let composition = step.composition.unwrap_or(default_composition);
            let step_style = step.resolve_style(element, context, base_style, resolver);
            let values = animating_properties
                .iter()
                .zip(base.iter())
                .map(|(property, base)| {
                    declared.contains(property).then(|| {
                        let value =
                            AnimationValue::from_computed_values(property, &step_style).unwrap();
                        composite_animation_value(base, value, composition)
                    })
                })
                .collect();
            keyframes.push(DeclaredKeyframe {
                position,
                timing_function,
                values,
            });
        }

        DeclaredKeyframes {
            keyframes: keyframes.into_boxed_slice(),
            base,
            automatic_timing,
        }
    }

    /// Whether a keyframe is attached to a named timeline range.
    fn has_range_keyframes(&self) -> bool {
        self.keyframes
            .iter()
            .any(|keyframe| matches!(keyframe.position, KeyframePosition::Range(..)))
    }

    /// One track per animating property, with the range keyframes placed by
    /// `ranges` and ignored without them. This is the one place a track is
    /// built.
    ///
    /// A property's track holds the keyframes declaring it by offset, equal
    /// offsets in computed keyframe order, plus an automatic keyframe at 0
    /// with the base value when none sits at or below 0, and one at 1 when
    /// none sits at or above 1 (scroll-animations-1 §5). A property every
    /// keyframe declaring it is ignored for is not animated: its track is
    /// empty.
    fn tracks(&self, ranges: Option<&TimelineRanges>) -> ArcSlice<PropertyTrack> {
        let mut placed: Vec<(f64, &DeclaredKeyframe)> = self
            .keyframes
            .iter()
            .filter_map(|keyframe| Some((keyframe.position.resolve(ranges)?, keyframe)))
            .collect();
        // A stable sort: equal offsets keep the computed keyframe order.
        // Every offset is finite.
        placed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        ArcSlice::from_iter(self.base.iter().enumerate().map(|(index, base)| {
            let mut keyframes: Vec<TrackKeyframe> = placed
                .iter()
                .filter_map(|&(offset, keyframe)| {
                    Some(TrackKeyframe {
                        offset,
                        timing_function: keyframe.timing_function.clone(),
                        value: keyframe.values[index].clone()?,
                    })
                })
                .collect();
            let declared = self
                .keyframes
                .iter()
                .any(|keyframe| keyframe.values[index].is_some());
            if keyframes.is_empty() && declared {
                return PropertyTrack(Box::new([]));
            }
            let automatic = |offset: f64, timing_function: &TimingFunction| TrackKeyframe {
                offset,
                timing_function: timing_function.clone(),
                value: base.clone(),
            };
            if keyframes
                .first()
                .is_none_or(|keyframe| keyframe.offset > 0.)
            {
                keyframes.insert(0, automatic(0., &self.automatic_timing[0]));
            }
            if keyframes.last().is_none_or(|keyframe| keyframe.offset < 1.) {
                keyframes.push(automatic(1., &self.automatic_timing[1]));
            }
            PropertyTrack(keyframes.into_boxed_slice())
        }))
    }
}

/// One keyframe of a [`PropertyTrack`].
#[derive(Clone, Debug, MallocSizeOf)]
struct TrackKeyframe {
    /// The keyframe offset: a fraction of the iteration, of any value.
    offset: f64,
    /// The timing function of the segments this keyframe starts.
    timing_function: TimingFunction,
    value: AnimationValue,
}

impl TrackKeyframe {
    fn data(&self) -> KeyframeDataForProperty<'_> {
        KeyframeDataForProperty {
            timing_function: &self.timing_function,
            start_percentage: self.offset,
            value: &self.value,
        }
    }
}

/// The keyframes of one animating property, ascending by offset, equal
/// offsets in computed keyframe order. Either empty, the property not
/// animated, or holding a keyframe at an offset at or below 0 and one at or
/// above 1, so every iteration offset in `[0, 1]` lies on a keyframe or
/// between two. Only [`DeclaredKeyframes::tracks`] builds one.
#[derive(Clone, Debug, MallocSizeOf)]
struct PropertyTrack(Box<[TrackKeyframe]>);

impl PropertyTrack {
    /// The value at simple iteration progress `progress` (negative in a
    /// backwards-filled before phase) through an iteration `duration` long,
    /// running reversed when `reversed`: the iteration offset is `progress`,
    /// or `1 - progress` reversed. `None` when the property is not animated.
    ///
    /// Between the last keyframe at or below the offset and the first above
    /// it, the value is interpolated with the keyframes' own offsets, eased by
    /// the lower keyframe's timing function running forward and by the upper
    /// one's over progress running from it to the lower one reversed; past
    /// the last keyframe it is the last one's value. So at equal offsets the
    /// value jumps to the later keyframe's. The before phase stands at offset
    /// 0 (1 reversed), and a reversed iteration's end at 0; a keyframe at that
    /// offset gives its value there without easing, as the before flag
    /// requires for `steps(jump-start)`.
    fn sample(&self, progress: f64, reversed: bool, duration: f64) -> Option<AnimationValue> {
        let keyframes = &*self.0;
        let (progress, held) = if progress < 0. {
            (0., Some(if reversed { 1. } else { 0. }))
        } else if reversed && progress == 1. {
            (1., Some(0.))
        } else {
            (progress, None)
        };
        if let Some(offset) = held {
            if let Some(keyframe) = keyframes
                .iter()
                .rev()
                .find(|keyframe| keyframe.offset == offset)
            {
                return Some(keyframe.value.clone());
            }
        }

        // The offset is compared as each direction reads it.
        let (lower, upper) = if reversed {
            let lower = keyframes
                .iter()
                .rposition(|keyframe| progress <= 1. - keyframe.offset)?;
            (lower, keyframes.get(lower + 1))
        } else {
            let upper = keyframes
                .iter()
                .position(|keyframe| progress < keyframe.offset);
            let lower = upper.unwrap_or(keyframes.len()).checked_sub(1)?;
            (lower, upper.map(|upper| &keyframes[upper]))
        };
        let lower = &keyframes[lower];
        let Some(upper) = upper else {
            return Some(lower.value.clone());
        };

        let between = (upper.offset - lower.offset).abs();
        let (from, to, start) = if reversed {
            (upper, lower, 1. - upper.offset)
        } else {
            (lower, upper, lower.offset)
        };
        Some(interpolate(
            &from.value,
            &to.value,
            &from.timing_function,
            between * duration,
            (progress - start) / between,
        ))
    }

    /// The consecutive pairs of keyframes whose span meets `[0, 1]`, pairs
    /// of equal offsets included: every value a sample at an iteration
    /// offset in `[0, 1]` can return is an endpoint of, or lies on, one of
    /// them.
    fn segments(&self) -> impl Iterator<Item = KeyframeSegment<'_>> {
        self.0
            .windows(2)
            .filter(|pair| pair[0].offset <= 1. && pair[1].offset > 0.)
            .map(|pair| KeyframeSegment {
                from: pair[0].data(),
                to: pair[1].data(),
            })
    }
}

/// An animation's keyframes: one track per animating property.
#[derive(Clone, MallocSizeOf)]
struct AnimationKeyframes {
    /// Indexed as [`Animation::animating_properties`] enumerates them; shared
    /// by clones.
    #[conditional_malloc_size_of]
    tracks: ArcSlice<PropertyTrack>,
    /// Present when a keyframe is attached to a named timeline range: what
    /// the tracks are rebuilt from when the ranges change.
    ranged: Option<RangedKeyframes>,
}

/// The keyframes of an animation with range keyframes.
#[derive(Clone, MallocSizeOf)]
struct RangedKeyframes {
    /// Shared by clones.
    #[conditional_malloc_size_of]
    declared: Arc<DeclaredKeyframes>,
    /// The ranges the tracks place the range keyframes with; `None` ignores
    /// them.
    ranges: Option<TimelineRanges>,
}

impl AnimationKeyframes {
    /// `declared`, with its range keyframes ignored until
    /// [`Animation::set_timeline_ranges`] places them. The declared keyframes
    /// are kept only where a later placement can rebuild the tracks.
    fn new(declared: DeclaredKeyframes) -> Self {
        let tracks = declared.tracks(None);
        let ranged = if declared.has_range_keyframes() {
            Some(RangedKeyframes {
                declared: Arc::new(declared),
                ranges: None,
            })
        } else {
            None
        };
        AnimationKeyframes { tracks, ranged }
    }
}

/// Composite a keyframe value with the underlying value according to the
/// given composite operation.
///
/// <https://drafts.csswg.org/web-animations-1/#applying-the-composite-operation>
fn composite_animation_value(
    underlying_value: &AnimationValue,
    keyframe_value: AnimationValue,
    composition: AnimationComposition,
) -> AnimationValue {
    let procedure = match composition {
        AnimationComposition::Replace => return keyframe_value,
        AnimationComposition::Add => Procedure::Add,
        AnimationComposition::Accumulate => Procedure::Accumulate { count: 1 },
    };
    underlying_value
        .animate(&keyframe_value, procedure)
        .unwrap_or(keyframe_value)
}

/// One keyframe that declares a given property.
#[derive(Clone, Copy, Debug)]
pub struct KeyframeDataForProperty<'a> {
    /// The timing function to use for transitions between this step
    /// and the next one in the iteration's direction: a forward iteration
    /// eases a segment by its lower keyframe's, a reverse one by its upper
    /// keyframe's.
    pub timing_function: &'a TimingFunction,

    /// The keyframe offset: the fraction of an iteration this keyframe sits
    /// at, outside `[0, 1]` for a range keyframe placed there.
    pub start_percentage: f64,

    /// The value this keyframe declares, or the base value for an automatic
    /// keyframe.
    pub value: &'a AnimationValue,
}

/// Two consecutive keyframes declaring one property: the segment an
/// animation interpolates that property over. A forward iteration eases it
/// by `from.timing_function`, a reverse one by `to.timing_function` over
/// progress running from `to` back to `from`.
#[derive(Clone, Copy, Debug)]
pub struct KeyframeSegment<'a> {
    /// The lower keyframe.
    pub from: KeyframeDataForProperty<'a>,
    /// The upper keyframe.
    pub to: KeyframeDataForProperty<'a>,
}

/// A CSS Animation
#[derive(Clone, MallocSizeOf)]
pub struct Animation {
    /// The name of this animation as defined by the style.
    pub name: Atom,

    /// The properties that change in this animation.
    properties_changed: PropertyDeclarationIdSet,

    /// The keyframes of this animation, one track per animating property.
    keyframes: AnimationKeyframes,

    /// The time this animation started at, which is the current value of the animation
    /// timeline when this animation was created plus any animation delay.
    pub started_at: f64,

    /// The duration of this animation; a nominal 1 for a progress-driven one.
    pub duration: f64,

    /// The delay of the animation; 0 for a progress-driven one.
    pub delay: f64,

    /// The `animation-fill-mode` property of this animation.
    pub fill_mode: AnimationFillMode,

    /// The current iteration state for the animation.
    pub iteration_state: KeyframesIterationState,

    /// Whether this animation is paused.
    pub state: AnimationState,

    /// The declared animation direction of this animation.
    pub direction: AnimationDirection,

    /// The current animation direction. This can only be `normal` or `reverse`.
    pub current_direction: AnimationDirection,

    /// The number of properties that are affected by this animation.
    pub number_of_animating_properties: usize,

    /// Whether or not this animation is new and or has already been tracked
    /// by the script thread.
    pub is_new: bool,

    /// The computed `animation-timeline` of this animation: `auto` for the
    /// document timeline, otherwise a scroll or view progress timeline.
    pub timeline: AnimationTimeline,

    /// A progress-driven animation's simple iteration progress and iteration
    /// direction, resolved by the embedder from its timeline (range, phase,
    /// fill and iteration applied; [`Self::sample_at`] applies the
    /// direction); `None` contributes nothing. Unread for an animation on the
    /// document timeline. The embedder writes the first sample even when the
    /// animation starts paused and holds it while paused, so one created
    /// under `animation-play-state: paused` shows the value at the scroll
    /// position of that moment rather than the base value.
    pub timeline_sample: Option<AnimationProgress>,
}

/// Whether `timeline` is `none`: an animation with no timeline is inactive.
fn is_none_timeline(timeline: &AnimationTimeline) -> bool {
    matches!(timeline, AnimationTimeline::Timeline(name) if name.value.is_none())
}

impl Animation {
    /// Whether this animation follows a progress timeline instead of the
    /// document timeline: its progress is `timeline_sample`, and the clock
    /// neither iterates nor ends it.
    pub fn is_progress_driven(&self) -> bool {
        !self.timeline.is_auto()
    }

    /// Whether the clock has to tick this animation.
    fn needs_to_be_ticked(&self) -> bool {
        !self.is_progress_driven() && self.state.needs_to_be_ticked()
    }

    /// Whether or not this animation is cancelled by changes from a new style.
    fn is_cancelled_in_new_style(&self, new_style: &Arc<ComputedValues>) -> bool {
        let new_ui = new_style.get_ui();
        let index = new_ui
            .animation_name_iter()
            .position(|animation_name| Some(&self.name) == animation_name.as_atom());
        let index = match index {
            Some(index) => index,
            None => return true,
        };

        let timeline = new_ui.animation_timeline_mod(index);
        if is_none_timeline(&timeline) {
            return true;
        }
        if !timeline.is_auto() {
            return false;
        }
        new_ui.animation_duration_mod(index).seconds() == 0.
    }

    /// Given the current time, advances this animation to the next iteration,
    /// updates times, and then toggles the direction if appropriate. Otherwise
    /// does nothing. Returns true if this animation has iterated.
    pub fn iterate_if_necessary(&mut self, time: f64) -> bool {
        if self.is_progress_driven() {
            return false;
        }
        if !self.iteration_over(time) {
            return false;
        }

        // Only iterate animations that are currently running.
        if self.state != AnimationState::Running {
            return false;
        }

        self.iterate_by(1.) == 1.
    }

    /// Advances a running animation past every whole iteration elapsed by
    /// `now`: the iteration count and direction a loop of
    /// `iterate_if_necessary(now)` reaches, with `started_at` equal to it up
    /// to f64 rounding (one `duration · n` step against repeated additions).
    /// One jump plus at most four checked single steps, so the cost does not
    /// grow with the iterations elapsed; idempotent at a fixed `now` unless
    /// `duration` is below the rounding of `started_at` or 2^52 iterations
    /// elapsed, where it stops short. Returns true if this animation has
    /// iterated.
    pub fn iterate_to(&mut self, now: f64) -> bool {
        if self.state != AnimationState::Running || self.is_progress_driven() {
            return false;
        }
        let mut cursor = self.cursor();
        let iterated = cursor.iterate_to(now, self.duration, self.direction);
        self.set_cursor(cursor);
        iterated
    }

    /// Attempts to advance this animation by `n` iterations, but stops when reaching
    /// the last iteration, and doesn't perform fractional iterations.
    /// Returns the actual number of iterations that happened.
    fn iterate_by(&mut self, n: f64) -> f64 {
        let mut cursor = self.cursor();
        let n = cursor.iterate_by(n, self.duration, self.direction);
        if n < 1. {
            return 0.;
        }

        if let AnimationState::Paused(ref mut progress) = self.state {
            debug_assert!(*progress >= n);
            *progress -= n;
        }

        self.set_cursor(cursor);
        n
    }

    /// A copy of the fields iterating moves.
    fn cursor(&self) -> IterationCursor {
        IterationCursor {
            started_at: self.started_at,
            iteration_state: self.iteration_state.clone(),
            current_direction: self.current_direction,
        }
    }

    fn set_cursor(&mut self, cursor: IterationCursor) {
        self.started_at = cursor.started_at;
        self.iteration_state = cursor.iteration_state;
        self.current_direction = cursor.current_direction;
    }

    /// A number (> 0 and <= 1) which represents the fraction of a full iteration
    /// that the current iteration of the animation lasts. This will be less than 1
    /// if the current iteration is the fractional remainder of a non-integral
    /// iteration count.
    pub fn current_iteration_end_progress(&self) -> f64 {
        self.cursor().current_iteration_end_progress()
    }

    /// The duration of the current iteration of this animation which may be less
    /// than the animation duration if it has a non-integral iteration count.
    pub fn current_iteration_duration(&self) -> f64 {
        self.current_iteration_end_progress() * self.duration
    }

    /// Whether or not the current iteration is over. Note that this method assumes that
    /// the animation is still running.
    fn iteration_over(&self, time: f64) -> bool {
        self.cursor().iteration_over(time, self.duration)
    }

    /// Whether or not this animation has finished at the provided time. This does
    /// not take into account canceling i.e. when an animation or transition is
    /// canceled due to changes in the style.
    pub fn has_ended(&self, time: f64) -> bool {
        self.has_ended_at(&self.cursor(), time)
    }

    /// [`Self::has_ended`] with the iteration fields of `cursor`.
    fn has_ended_at(&self, cursor: &IterationCursor, time: f64) -> bool {
        if self.is_progress_driven() || !cursor.on_last_iteration() {
            return false;
        }

        match self.state {
            AnimationState::Finished => true,
            AnimationState::Paused(progress) => progress >= cursor.current_iteration_end_progress(),
            AnimationState::Running => cursor.iteration_ended(time, self.duration),
            AnimationState::Pending | AnimationState::Canceled => false,
        }
    }

    /// Whether this animation, running and iterated to `time` as
    /// [`Self::progress_at`] iterates it, has ended at `time`.
    fn run_ended_at(&self, time: f64) -> bool {
        let mut cursor = self.cursor();
        cursor.iterate_to(time, self.duration, self.direction);
        cursor.on_last_iteration() && cursor.iteration_ended(time, self.duration)
    }

    /// The first instant a finite running or pending animation, iterated as
    /// [`Self::progress_at`] iterates it, has ended at: it contributes at the
    /// instant before, and `has_ended` holds from it on after `iterate_to`.
    /// `None` for one that never ends on its own: infinite, paused, finished,
    /// canceled or progress-driven.
    pub fn expires_at(&self) -> Option<f64> {
        // Rounding puts that instant within a few ulps of the estimate.
        const ULPS: usize = 8;

        if self.is_progress_driven() {
            return None;
        }
        match (&self.state, &self.iteration_state) {
            (
                AnimationState::Running | AnimationState::Pending,
                KeyframesIterationState::Finite(..),
            ) => {},
            _ => return None,
        }
        let mut last = self.cursor();
        let before_last = last.remaining_iterations().ceil() - 1.;
        last.iterate_by(before_last, self.duration, self.direction);
        let estimate = last.started_at + self.duration * last.current_iteration_end_progress();
        let mut cursor = self.cursor();
        cursor.iterate_to(estimate, self.duration, self.direction);
        let mut end = cursor.started_at + self.duration * cursor.current_iteration_end_progress();
        for _ in 0..ULPS {
            if self.run_ended_at(end) {
                break;
            }
            end = end.next_up();
        }
        for _ in 0..ULPS {
            if !self.run_ended_at(end.next_down()) {
                break;
            }
            end = end.next_down();
        }
        Some(end)
    }

    /// The properties this animation's keyframes animate, each with the
    /// index [`Self::keyframe_segments`] takes. A property every keyframe
    /// declaring it is ignored for is not one of them.
    pub fn animating_properties(&self) -> impl Iterator<Item = (usize, PropertyDeclarationId<'_>)> {
        self.keyframes
            .tracks
            .iter()
            .enumerate()
            .filter_map(|(index, track)| Some((index, track.0.first()?.value.id())))
    }

    /// The segments of the property at `property_index` that a sample at an
    /// iteration offset in `[0, 1]` can reach: pairs of consecutive keyframes
    /// of its track, ascending, pairs of equal offsets included. Every value
    /// such a sample can return is an endpoint of, or lies on, one of them.
    pub fn keyframe_segments(
        &self,
        property_index: usize,
    ) -> impl Iterator<Item = KeyframeSegment<'_>> {
        self.keyframes
            .tracks
            .get(property_index)
            .into_iter()
            .flat_map(PropertyTrack::segments)
    }

    /// Whether a keyframe of this animation is attached to a named timeline
    /// range (scroll-animations-1 §5): [`Self::set_timeline_ranges`] places
    /// it.
    pub fn has_range_keyframes(&self) -> bool {
        self.keyframes.ranged.is_some()
    }

    /// Places the range keyframes with `ranges`, the named ranges of the
    /// animation's timeline as fractions of its attachment range, or ignores
    /// them with `None`. Rebuilds the keyframe tracks when `ranges` differs
    /// from the ranges they were built with, and answers whether it did. An
    /// animation without range keyframes has nothing to place.
    pub fn set_timeline_ranges(&mut self, ranges: Option<TimelineRanges>) -> bool {
        let Some(ranged) = &mut self.keyframes.ranged else {
            return false;
        };
        if ranged.ranges == ranges {
            return false;
        }
        self.keyframes.tracks = ranged.declared.tracks(ranges.as_ref());
        ranged.ranges = ranges;
        true
    }

    /// The ranges the range keyframes are placed with.
    fn timeline_ranges(&self) -> Option<TimelineRanges> {
        self.keyframes.ranged.as_ref()?.ranges
    }

    /// Updates the appropiate state from other animation.
    ///
    /// This happens when an animation is re-submitted to layout, presumably
    /// because of an state change.
    ///
    /// There are some bits of state we can't just replace, over all taking in
    /// account times, so here's that logic.
    pub fn update_from_other(&mut self, other: &Self, now: f64) {
        use self::AnimationState::*;

        debug!(
            "KeyframesAnimationState::update_from_other({:?}, {:?})",
            self, other
        );

        // A progress-driven animation has no time state to carry over, only
        // what the embedder last wrote: the progress, and the ranges its
        // range keyframes are placed with, which place the new keyframes
        // until the embedder resolves them again. `maybe_start_animations`
        // replaces an animation whose timeline kind changed.
        if other.is_progress_driven() {
            debug_assert_eq!(self.is_progress_driven(), other.is_progress_driven());
            let timeline_sample = self.timeline_sample;
            let timeline_ranges = self.timeline_ranges();
            *self = other.clone();
            self.timeline_sample = timeline_sample;
            self.set_timeline_ranges(timeline_ranges);
            return;
        }

        // NB: We shall not touch the started_at field, since we don't want to
        // restart the animation.
        let old_started_at = self.started_at;
        let old_delay = self.delay;
        let old_duration = self.duration;
        let old_direction = self.current_direction;
        let old_state = self.state.clone();
        let old_iteration_state = self.iteration_state.clone();

        *self = other.clone();
        self.current_direction = old_direction;

        if self.delay != old_delay {
            // `started_at` incorporates the delay, so changing the delay necessarily changes `started_at`.
            // Note: `started_at` may actually be in the future.
            self.started_at = old_started_at + (self.delay - old_delay);

            match old_state {
                Paused(old_progress) => {
                    let mut progress = old_progress + (old_delay - self.delay) / self.duration;
                    progress -= self.iterate_by(progress);
                    self.state = Paused(progress);
                },
                Finished => {
                    if self.has_ended(now) {
                        self.state = Finished;
                    } else if self.started_at <= now {
                        self.state = Running;
                    } else {
                        self.state = Pending;
                    }
                },
                Canceled | Pending | Running => {
                    // Re-advance iterations from a fresh iteration state.
                    let new_starting_progress = (now - self.started_at) / self.duration;
                    match self.iteration_state {
                        KeyframesIterationState::Finite(ref mut current, _) => *current = 0.0,
                        _ => {},
                    }
                    if let AnimationState::Paused(starting_progress) = &mut self.state {
                        *starting_progress = new_starting_progress;
                    }
                    self.iterate_by(new_starting_progress);
                },
            }

            // Don't check old_state when delay changed.
            if self.state == Pending && self.started_at <= now {
                self.state = Running;
            }
        } else {
            self.started_at = old_started_at;

            // Don't update the iteration count, just the iteration limit.
            // TODO: see how changing the limit affects rendering in other browsers.
            // We might need to keep the iteration count even when it's infinite.
            match (&mut self.iteration_state, old_iteration_state) {
                (
                    &mut KeyframesIterationState::Finite(ref mut iters, _),
                    KeyframesIterationState::Finite(old_iters, _),
                ) => *iters = old_iters,
                _ => {},
            }

            // Don't pause or restart animations that should remain finished.
            // We call mem::replace because `has_ended(...)` looks at `Animation::state`.
            let new_state = std::mem::replace(&mut self.state, Running);
            if old_state == Finished && self.has_ended(now) {
                self.state = Finished;
            } else {
                self.state = new_state;
            }

            // If we're unpausing the animation, fake the start time so we seem to
            // restore it.
            //
            // If the animation keeps paused, keep the old value.
            //
            // If we're pausing the animation, compute the progress value.
            match (&mut self.state, &old_state) {
                (&mut Pending, &Paused(progress)) => {
                    self.started_at = now - (self.duration * progress);
                },
                (&mut Paused(ref mut new), &Paused(old)) => *new = old,
                (&mut Paused(ref mut progress), &Running) => {
                    *progress = (now - old_started_at) / old_duration
                },
                _ => {},
            }

            // Try to detect when we should skip straight to the running phase to
            // avoid sending multiple animationstart events.
            if self.state == Pending && self.started_at <= now && old_state != Pending {
                self.state = Running;
            }
        }
    }

    /// Fill in an `AnimationValueMap` with values calculated from this animation at
    /// the given time value.
    fn get_property_declaration_at_time(&self, now: f64, map: &mut AnimationValueMap) {
        if let Some(at) = self.progress_of(&self.cursor(), now) {
            self.sample_at(at, map);
        }
    }

    /// Where this animation stands at `now`, without moving `self`: its
    /// iteration fields as `iterate_to(now)` leaves them, read as the cascade
    /// reads them. `None` when the animation contributes nothing at `now`.
    pub fn progress_at(&self, now: f64) -> Option<AnimationProgress> {
        let mut cursor = self.cursor();
        if self.state == AnimationState::Running && !self.is_progress_driven() {
            cursor.iterate_to(now, self.duration, self.direction);
        }
        self.progress_of(&cursor, now)
    }

    /// Where this animation stands at `now` with the iteration fields of
    /// `cursor`: the fill and `has_ended` rules applied, the progress clamped
    /// to the current iteration unless a backwards fill reads it negative. A
    /// progress-driven animation stands at its `timeline_sample`, paused or
    /// not.
    fn progress_of(&self, cursor: &IterationCursor, now: f64) -> Option<AnimationProgress> {
        if self.keyframes.tracks.is_empty() {
            // Nothing to do.
            return None;
        }
        if self.is_progress_driven() {
            return match self.state {
                AnimationState::Canceled => None,
                _ => self.timeline_sample,
            };
        }

        // Raw progress ratio of the animation: can be negative (before start) or
        // >1.0 (after end or during multiple iterations).
        let progress = match self.state {
            AnimationState::Running | AnimationState::Pending | AnimationState::Finished => {
                (now - cursor.started_at) / self.duration
            },
            AnimationState::Paused(progress) => progress,
            AnimationState::Canceled => return None,
        };

        if progress < 0.
            && self.fill_mode != AnimationFillMode::Backwards
            && self.fill_mode != AnimationFillMode::Both
        {
            return None;
        }
        if self.has_ended_at(cursor, now)
            && self.fill_mode != AnimationFillMode::Forwards
            && self.fill_mode != AnimationFillMode::Both
        {
            return None;
        }

        let progress = if progress < 0.0 {
            progress
        } else {
            // Progress clamped to the current iteration [0.0, 1.0].
            progress
                .min(cursor.current_iteration_end_progress())
                .max(0.0)
        };
        Some(AnimationProgress {
            progress,
            direction: cursor.current_direction,
        })
    }

    /// Fill in an `AnimationValueMap` with this animation's values at `at`,
    /// applying its direction: each animated property's track sampled at the
    /// iteration offset `at` reads (see `PropertyTrack::sample`).
    pub fn sample_at(&self, at: AnimationProgress, map: &mut AnimationValueMap) {
        let reversed = match at.direction {
            AnimationDirection::Normal => false,
            AnimationDirection::Reverse => true,
            _ => unreachable!("Current animation direction can only be `normal` or `reverse`."),
        };
        for track in self.keyframes.tracks.iter() {
            if let Some(value) = track.sample(at.progress, reversed, self.duration) {
                map.insert(value.id().to_owned(), value);
            }
        }
    }
}

impl fmt::Debug for Animation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Animation")
            .field("name", &self.name)
            .field("started_at", &self.started_at)
            .field("duration", &self.duration)
            .field("delay", &self.delay)
            .field("iteration_state", &self.iteration_state)
            .field("state", &self.state)
            .field("direction", &self.direction)
            .field("current_direction", &self.current_direction)
            .field("timeline", &self.timeline)
            .field("timeline_sample", &self.timeline_sample)
            .field("cascade_style", &())
            .finish()
    }
}

/// A CSS Transition
#[derive(Clone, Debug, MallocSizeOf)]
pub struct Transition {
    /// The start time of this transition, which is the current value of the animation
    /// timeline when this transition was created plus any animation delay.
    pub start_time: f64,

    /// The delay used for this transition.
    pub delay: f64,

    /// The internal style `PropertyAnimation` for this transition.
    pub property_animation: PropertyAnimation,

    /// The state of this transition.
    pub state: AnimationState,

    /// Whether or not this transition is new and or has already been tracked
    /// by the script thread.
    pub is_new: bool,

    /// If this `Transition` has been replaced by a new one this field is
    /// used to help produce better reversed transitions.
    pub reversing_adjusted_start_value: AnimationValue,

    /// If this `Transition` has been replaced by a new one this field is
    /// used to help produce better reversed transitions.
    pub reversing_shortening_factor: f64,
}

impl Transition {
    fn new(
        start_time: f64,
        delay: f64,
        duration: f64,
        from: AnimationValue,
        to: AnimationValue,
        timing_function: &TimingFunction,
    ) -> Self {
        let property_animation = PropertyAnimation {
            from: from.clone(),
            to,
            timing_function: timing_function.clone(),
            duration,
        };
        Self {
            start_time,
            delay,
            property_animation,
            state: AnimationState::Pending,
            is_new: true,
            reversing_adjusted_start_value: from,
            reversing_shortening_factor: 1.0,
        }
    }

    fn update_for_possibly_reversed_transition(
        &mut self,
        replaced_transition: &Transition,
        delay: f64,
        now: f64,
    ) {
        // If we reach here, we need to calculate a reversed transition according to
        // https://drafts.csswg.org/css-transitions/#starting
        //
        //  "...if the reversing-adjusted start value of the running transition
        //  is the same as the value of the property in the after-change style (see
        //  the section on reversing of transitions for why these case exists),
        //  implementations must cancel the running transition and start
        //  a new transition..."
        if replaced_transition.reversing_adjusted_start_value != self.property_animation.to {
            return;
        }

        // "* reversing-adjusted start value is the end value of the running transition"
        let replaced_animation = &replaced_transition.property_animation;
        self.reversing_adjusted_start_value = replaced_animation.to.clone();

        // "* reversing shortening factor is the absolute value, clamped to the
        //    range [0, 1], of the sum of:
        //    1. the output of the timing function of the old transition at the
        //      time of the style change event, times the reversing shortening
        //      factor of the old transition
        //    2.  1 minus the reversing shortening factor of the old transition."
        let transition_progress = ((now - replaced_transition.start_time)
            / (replaced_transition.property_animation.duration))
            .min(1.0)
            .max(0.0);
        let timing_function_output = replaced_animation.timing_function_output(transition_progress);
        let old_reversing_shortening_factor = replaced_transition.reversing_shortening_factor;
        self.reversing_shortening_factor = ((timing_function_output
            * old_reversing_shortening_factor)
            + (1.0 - old_reversing_shortening_factor))
            .abs()
            .min(1.0)
            .max(0.0);

        // "* start time is the time of the style change event plus:
        //    1. if the matching transition delay is nonnegative, the matching
        //       transition delay, or.
        //    2. if the matching transition delay is negative, the product of the new
        //       transition’s reversing shortening factor and the matching transition delay,"
        self.start_time = if delay >= 0. {
            now + delay
        } else {
            now + (self.reversing_shortening_factor * delay)
        };

        // "* end time is the start time plus the product of the matching transition
        //    duration and the new transition’s reversing shortening factor,"
        self.property_animation.duration *= self.reversing_shortening_factor;

        // "* start value is the current value of the property in the running transition,
        //  * end value is the value of the property in the after-change style,"
        let procedure = Procedure::Interpolate {
            progress: timing_function_output,
        };
        match replaced_animation
            .from
            .animate(&replaced_animation.to, procedure)
        {
            Ok(new_start) => self.property_animation.from = new_start,
            Err(..) => {},
        }
    }

    /// Whether or not this animation has ended at the provided time. This does
    /// not take into account canceling i.e. when an animation or transition is
    /// canceled due to changes in the style.
    pub fn has_ended(&self, time: f64) -> bool {
        time >= self.start_time + (self.property_animation.duration)
    }

    /// Update the given animation at a given point of progress.
    pub fn calculate_value(&self, time: f64) -> AnimationValue {
        let progress = if time < self.start_time {
            0.0
        } else if self.property_animation.duration == 0.0 {
            1.0
        } else {
            ((time - self.start_time) / self.property_animation.duration).clamp(0.0, 1.0)
        };

        self.property_animation.calculate_value(progress)
    }
}

/// Holds the animation state for a particular element.
#[derive(Debug, Default, MallocSizeOf)]
pub struct ElementAnimationSet {
    /// The animations for this element.
    pub animations: Vec<Animation>,

    /// The transitions for this element.
    pub transitions: Vec<Transition>,

    /// Whether or not this ElementAnimationSet has had animations or transitions
    /// which have been added, removed, or had their state changed.
    pub dirty: bool,
}

impl ElementAnimationSet {
    /// Cancel all animations in this `ElementAnimationSet`. This is typically called
    /// when the element has been removed from the DOM.
    pub fn cancel_all_animations(&mut self) {
        self.dirty = !self.animations.is_empty();
        for animation in self.animations.iter_mut() {
            animation.state = AnimationState::Canceled;
        }
        self.cancel_active_transitions();
    }

    fn cancel_active_transitions(&mut self) {
        for transition in self.transitions.iter_mut() {
            if transition.state != AnimationState::Finished {
                self.dirty = true;
                transition.state = AnimationState::Canceled;
            }
        }
    }

    /// Apply all active animations.
    pub fn apply_active_animations(
        &self,
        context: &SharedStyleContext,
        style: &mut Arc<ComputedValues>,
    ) {
        let now = context.current_time_for_animations;
        let mutable_style = Arc::make_mut(style);
        if let Some(map) = self.get_value_map_for_active_animations(now) {
            for value in map.values() {
                value.set_in_style_for_servo(mutable_style, context);
            }
        }

        if let Some(map) = self.get_value_map_for_transitions(now, IgnoreTransitions::Canceled) {
            for value in map.values() {
                value.set_in_style_for_servo(mutable_style, context);
            }
        }
    }

    /// Clear all canceled animations and transitions from this `ElementAnimationSet`.
    pub fn clear_canceled_animations(&mut self) {
        self.animations
            .retain(|animation| animation.state != AnimationState::Canceled);
        self.transitions
            .retain(|animation| animation.state != AnimationState::Canceled);
    }

    /// Whether this `ElementAnimationSet` is empty, which means it doesn't
    /// hold any animations in any state.
    pub fn is_empty(&self) -> bool {
        self.animations.is_empty() && self.transitions.is_empty()
    }

    /// Whether or not this state needs animation ticks for its transitions
    /// or animations. A progress-driven animation never does.
    pub fn needs_animation_ticks(&self) -> bool {
        self.animations
            .iter()
            .any(|animation| animation.needs_to_be_ticked())
            || self
                .transitions
                .iter()
                .any(|transition| transition.state.needs_to_be_ticked())
    }

    /// The number of running animations and transitions for this
    /// `ElementAnimationSet`, progress-driven animations excluded.
    pub fn running_animation_and_transition_count(&self) -> usize {
        self.animations
            .iter()
            .filter(|animation| animation.needs_to_be_ticked())
            .count()
            + self
                .transitions
                .iter()
                .filter(|transition| transition.state.needs_to_be_ticked())
                .count()
    }

    /// If this `ElementAnimationSet` has any any active animations.
    pub fn has_active_animation(&self) -> bool {
        self.animations
            .iter()
            .any(|animation| animation.state != AnimationState::Canceled)
    }

    /// If this `ElementAnimationSet` has any any active transitions.
    pub fn has_active_transition(&self) -> bool {
        self.transitions
            .iter()
            .any(|transition| transition.state != AnimationState::Canceled)
    }

    /// Update our animations given a new style, canceling or starting new animations
    /// when appropriate.
    pub fn update_animations_for_new_style<E>(
        &mut self,
        element: E,
        context: &SharedStyleContext,
        new_style: &Arc<ComputedValues>,
        resolver: &mut StyleResolverForElement<E>,
    ) where
        E: TElement,
    {
        for animation in self.animations.iter_mut() {
            if animation.is_cancelled_in_new_style(new_style) {
                animation.state = AnimationState::Canceled;
            }
        }

        maybe_start_animations(element, &context, &new_style, self, resolver);
    }

    /// Update our transitions given a new style, canceling or starting new animations
    /// when appropriate.
    pub fn update_transitions_for_new_style(
        &mut self,
        might_need_transitions_update: bool,
        context: &SharedStyleContext,
        old_style: Option<&Arc<ComputedValues>>,
        after_change_style: &Arc<ComputedValues>,
    ) {
        // If this is the first style, we don't trigger any transitions and we assume
        // there were no previously triggered transitions.
        let mut before_change_style = match old_style {
            Some(old_style) => Arc::clone(old_style),
            None => return,
        };

        // If the style of this element is display:none, then cancel all active transitions.
        if after_change_style.get_box().get_display().is_none() {
            self.cancel_active_transitions();
            return;
        }

        if !might_need_transitions_update {
            return;
        }

        // We convert old values into `before-change-style` here.
        if self.has_active_transition() || self.has_active_animation() {
            self.apply_active_animations(context, &mut before_change_style);
        }

        let transitioning_properties = start_transitions_if_applicable(
            context,
            &before_change_style,
            after_change_style,
            self,
        );

        // Cancel any non-finished transitions that have properties which no
        // longer transition.
        //
        // Step 3 in https://drafts.csswg.org/css-transitions/#starting:
        // > If the element has a running transition or completed transition for
        // > the property, and there is not a matching transition-property value,
        // > then implementations must cancel the running transition or remove the
        // > completed transition from the set of completed transitions.
        //
        // TODO: This is happening here as opposed to in
        // `start_transition_if_applicable` as an optimization, but maybe this
        // code should be reworked to be more like the specification.
        for transition in self.transitions.iter_mut() {
            if transition.state == AnimationState::Finished
                || transition.state == AnimationState::Canceled
            {
                continue;
            }
            if transitioning_properties.contains(transition.property_animation.property_id()) {
                continue;
            }
            transition.state = AnimationState::Canceled;
            self.dirty = true;
        }
    }

    fn start_transition_if_applicable(
        &mut self,
        context: &SharedStyleContext,
        property_declaration_id: &PropertyDeclarationId,
        index: usize,
        old_style: &ComputedValues,
        new_style: &Arc<ComputedValues>,
    ) {
        let style = new_style.get_ui();
        let allow_discrete =
            style.transition_behavior_mod(index) == TransitionBehavior::AllowDiscrete;

        // FIXME(emilio): Handle the case where old_style and new_style's writing mode differ.
        let Some(from) = AnimationValue::from_computed_values(*property_declaration_id, old_style)
        else {
            return;
        };
        let Some(to) = AnimationValue::from_computed_values(*property_declaration_id, new_style)
        else {
            return;
        };

        let timing_function = style.transition_timing_function_mod(index);
        let duration = style.transition_duration_mod(index).seconds() as f64;
        let delay = style.transition_delay_mod(index).seconds() as f64;
        let now = context.current_time_for_animations;
        let transitionable = property_declaration_id.is_animatable()
            && (allow_discrete || !property_declaration_id.is_discrete_animatable())
            && (allow_discrete || from.interpolable_with(&to));

        let mut existing_transition = self.transitions.iter_mut().find(|transition| {
            transition.property_animation.property_id() == *property_declaration_id
        });

        // Step 1:
        // > If all of the following are true:
        // >  - the element does not have a running transition for the property,
        // >  - the before-change style is different from the after-change style
        // >    for that property, and the values for the property are
        // >    transitionable,
        // >  - the element does not have a completed transition for the property
        // >    or the end value of the completed transition is different from the
        // >    after-change style for the property,
        // >  - there is a matching transition-property value, and
        // >  - the combined duration is greater than 0s,
        //
        // This function is only run if there is a matching transition-property
        // value, so that check is skipped here.
        let has_running_transition = existing_transition.as_ref().is_some_and(|transition| {
            transition.state != AnimationState::Finished
                && transition.state != AnimationState::Canceled
        });
        let no_completed_transition_or_end_values_differ =
            existing_transition.as_ref().is_none_or(|transition| {
                transition.state != AnimationState::Finished
                    || transition.property_animation.to != to
            });
        if !has_running_transition
            && from != to
            && transitionable
            && no_completed_transition_or_end_values_differ
            && (duration + delay > 0.0)
        {
            // > then implementations must remove the completed transition (if
            // > present) from the set of completed transitions and start a
            // > transition whose:
            // >
            // > - start time is the time of the style change event plus the matching transition delay,
            // > - end time is the start time plus the matching transition duration,
            // > - start value is the value of the transitioning property in the before-change style,
            // > - end value is the value of the transitioning property in the after-change style,
            // > - reversing-adjusted start value is the same as the start value, and
            // > - reversing shortening factor is 1.
            self.transitions.push(Transition::new(
                now + delay, /* start_time */
                delay,
                duration,
                from,
                to,
                &timing_function,
            ));
            self.dirty = true;
            return;
        }

        // > Step 2: Otherwise, if the element has a completed transition for the
        // > property and the end value of the completed transition is different
        // > from the after-change style for the property, then implementations
        // > must remove the completed transition from the set of completed
        // > transitions.
        //
        // All completed transitions will be cleared from the `AnimationSet` in
        // `process_animations_for_style in `matching.rs`.

        // > Step 3: If the element has a running transition or completed
        // > transition for the property, and there is not a matching
        // > transition-property value, then implementations must cancel the
        // > running transition or remove the completed transition from the set
        // > of completed transitions.
        //
        // - All completed transitions will be cleared cleared from the `AnimationSet` in
        //   `process_animations_for_style in `matching.rs`.
        // - Transitions for properties that don't have a matching transition-property
        //   value will be canceled in `Self::update_transitions_for_new_style`. In addition,
        //   this method is only called for properties that do ahave a matching
        //   transition-property value.

        let Some(existing_transition) = existing_transition.as_mut() else {
            return;
        };

        // > Step 4: If the element has a running transition for the property,
        // > there is a matching transition-property value, and the end value of
        // > the running transition is not equal to the value of the property in
        // > the after-change style, then:
        if has_running_transition && existing_transition.property_animation.to != to {
            // > Step 4.1: If the current value of the property in the running transition is
            // > equal to the value of the property in the after-change style, or
            // > if these two values are not transitionable, then implementations
            // > must cancel the running transition.
            let current_value = existing_transition.calculate_value(now);
            let transitionable_from_current_value =
                transitionable && (allow_discrete || current_value.interpolable_with(&to));
            if current_value == to || !transitionable_from_current_value {
                existing_transition.state = AnimationState::Canceled;
                self.dirty = true;
                return;
            }

            // > Step 4.2: Otherwise, if the combined duration is less than or
            // > equal to 0s, or if the current value of the property in the
            // > running transition is not transitionable with the value of the
            // > property in the after-change style, then implementations must
            // > cancel the running transition.
            if duration + delay <= 0.0 {
                existing_transition.state = AnimationState::Canceled;
                self.dirty = true;
                return;
            }

            // > Step 4.3: Otherwise, if the reversing-adjusted start value of the
            // > running transition is the same as the value of the property in
            // > the after-change style (see the section on reversing of
            // > transitions for why these case exists), implementations must
            // > cancel the running transition and start a new transition whose:
            if existing_transition.reversing_adjusted_start_value == to {
                existing_transition.state = AnimationState::Canceled;

                let mut transition = Transition::new(
                    now + delay, /* start_time */
                    delay,
                    duration,
                    from,
                    to,
                    &timing_function,
                );

                // This function takes care of applying all of the modifications to the transition
                // after "whose:" above.
                transition.update_for_possibly_reversed_transition(
                    &existing_transition,
                    delay,
                    now,
                );

                self.transitions.push(transition);
                self.dirty = true;
                return;
            }

            // > Step 4.4: Otherwise, implementations must cancel the running
            // > transition and start a new transition whose:
            // >  - start time is the time of the style change event plus the matching transition delay,
            // >  - end time is the start time plus the matching transition duration,
            // >  - start value is the current value of the property in the running transition,
            // >  - end value is the value of the property in the after-change style,
            // >  - reversing-adjusted start value is the same as the start value, and
            // >  - reversing shortening factor is 1.
            existing_transition.state = AnimationState::Canceled;
            self.transitions.push(Transition::new(
                now + delay, /* start_time */
                delay,
                duration,
                current_value,
                to,
                &timing_function,
            ));
            self.dirty = true;
        }
    }

    /// Generate a `AnimationValueMap` for this `ElementAnimationSet`'s
    /// transitions, ignoring those specified by the `ignore_transitions`
    /// argument.
    fn get_value_map_for_transitions(
        &self,
        now: f64,
        ignore_transitions: IgnoreTransitions,
    ) -> Option<AnimationValueMap> {
        if !self.has_active_transition() {
            return None;
        }

        let mut map =
            AnimationValueMap::with_capacity_and_hasher(self.transitions.len(), Default::default());
        for transition in &self.transitions {
            match ignore_transitions {
                IgnoreTransitions::Canceled => {
                    if transition.state == AnimationState::Canceled {
                        continue;
                    }
                },
                IgnoreTransitions::CanceledAndFinished => {
                    if transition.state == AnimationState::Canceled
                        || transition.state == AnimationState::Finished
                    {
                        continue;
                    }
                },
            }

            let value = transition.calculate_value(now);
            map.insert(value.id().to_owned(), value);
        }

        Some(map)
    }

    /// Generate a `AnimationValueMap` for this `ElementAnimationSet`'s
    /// active animations at the given time value.
    pub fn get_value_map_for_active_animations(&self, now: f64) -> Option<AnimationValueMap> {
        if !self.has_active_animation() {
            return None;
        }

        let mut map = Default::default();
        for animation in &self.animations {
            animation.get_property_declaration_at_time(now, &mut map);
        }

        Some(map)
    }
}

#[derive(Clone, Debug, Eq, Hash, MallocSizeOf, PartialEq)]
/// A key that is used to identify nodes in the `DocumentAnimationSet`.
pub struct AnimationSetKey {
    /// The node for this `AnimationSetKey`.
    pub node: OpaqueNode,
    /// The pseudo element for this `AnimationSetKey`. If `None` this key will
    /// refer to the main content for its node.
    pub pseudo_element: Option<PseudoElement>,
}

impl AnimationSetKey {
    /// Create a new key given a node and optional pseudo element.
    pub fn new(node: OpaqueNode, pseudo_element: Option<PseudoElement>) -> Self {
        AnimationSetKey {
            node,
            pseudo_element,
        }
    }

    /// Create a new key for the main content of this node.
    pub fn new_for_non_pseudo(node: OpaqueNode) -> Self {
        AnimationSetKey {
            node,
            pseudo_element: None,
        }
    }

    /// Create a new key for given node and pseudo element.
    pub fn new_for_pseudo(node: OpaqueNode, pseudo_element: PseudoElement) -> Self {
        AnimationSetKey {
            node,
            pseudo_element: Some(pseudo_element),
        }
    }
}

#[derive(Clone, Debug, Default, MallocSizeOf)]
/// A set of animations for a document.
pub struct DocumentAnimationSet {
    /// The `ElementAnimationSet`s that this set contains.
    #[ignore_malloc_size_of = "Arc is hard"]
    pub sets: Arc<RwLock<FxHashMap<AnimationSetKey, ElementAnimationSet>>>,
}

impl DocumentAnimationSet {
    /// Return whether or not the provided node has active CSS animations.
    pub fn has_active_animations(&self, key: &AnimationSetKey) -> bool {
        self.sets
            .read()
            .get(key)
            .map_or(false, |set| set.has_active_animation())
    }

    /// Return whether or not the provided node has active CSS transitions.
    pub fn has_active_transitions(&self, key: &AnimationSetKey) -> bool {
        self.sets
            .read()
            .get(key)
            .map_or(false, |set| set.has_active_transition())
    }

    /// Return a locked PropertyDeclarationBlock with animation values for the given
    /// key and time.
    pub fn get_animation_declarations(
        &self,
        key: &AnimationSetKey,
        time: f64,
        shared_lock: &SharedRwLock,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        self.sets
            .read()
            .get(key)
            .and_then(|set| set.get_value_map_for_active_animations(time))
            .map(|map| {
                let block = PropertyDeclarationBlock::from_animation_value_map(&map);
                Arc::new(shared_lock.wrap(block))
            })
    }

    /// Return a locked PropertyDeclarationBlock with transition values for the given
    /// key and time.
    pub fn get_transition_declarations(
        &self,
        key: &AnimationSetKey,
        time: f64,
        shared_lock: &SharedRwLock,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        self.sets
            .read()
            .get(key)
            .and_then(|set| {
                set.get_value_map_for_transitions(time, IgnoreTransitions::CanceledAndFinished)
            })
            .map(|map| {
                let block = PropertyDeclarationBlock::from_animation_value_map(&map);
                Arc::new(shared_lock.wrap(block))
            })
    }

    /// Get all the animation declarations for the given key, returning an empty
    /// `AnimationDeclarations` if there are no animations.
    pub fn get_all_declarations(
        &self,
        key: &AnimationSetKey,
        time: f64,
        shared_lock: &SharedRwLock,
    ) -> AnimationDeclarations {
        let sets = self.sets.read();
        let set = match sets.get(key) {
            Some(set) => set,
            None => return Default::default(),
        };

        let animations = set.get_value_map_for_active_animations(time).map(|map| {
            let block = PropertyDeclarationBlock::from_animation_value_map(&map);
            Arc::new(shared_lock.wrap(block))
        });
        let transitions = set
            .get_value_map_for_transitions(time, IgnoreTransitions::CanceledAndFinished)
            .map(|map| {
                let block = PropertyDeclarationBlock::from_animation_value_map(&map);
                Arc::new(shared_lock.wrap(block))
            });
        AnimationDeclarations {
            animations,
            transitions,
        }
    }

    /// Cancel all animations for set at the given key.
    pub fn cancel_all_animations_for_key(&self, key: &AnimationSetKey) {
        if let Some(set) = self.sets.write().get_mut(key) {
            set.cancel_all_animations();
        }
    }
}

/// Kick off any new transitions for this node and return all of the properties that are
/// transitioning. This is at the end of calculating style for a single node.
pub fn start_transitions_if_applicable(
    context: &SharedStyleContext,
    old_style: &ComputedValues,
    new_style: &Arc<ComputedValues>,
    animation_state: &mut ElementAnimationSet,
) -> PropertyDeclarationIdSet {
    // See <https://www.w3.org/TR/css-transitions-1/#transitions>
    // "If a property is specified multiple times in the value of transition-property
    // (either on its own, via a shorthand that contains it, or via the all value),
    // then the transition that starts uses the duration, delay, and timing function
    // at the index corresponding to the last item in the value of transition-property
    // that calls for animating that property."
    // See Example 3 of <https://www.w3.org/TR/css-transitions-1/#transitions>
    //
    // Reversing the transition order here means that transitions defined later in the list
    // have preference, in accordance with the specification.
    //
    // TODO: It would be better to be able to do this without having to allocate an array.
    // We should restructure the code or make `transition_properties()` return a reversible
    // iterator in order to avoid the allocation.
    let mut transition_properties = new_style.transition_properties().collect::<Vec<_>>();
    transition_properties.reverse();

    let mut properties_that_transition = PropertyDeclarationIdSet::default();
    for transition in transition_properties {
        let physical_property = transition
            .property
            .as_borrowed()
            .to_physical(new_style.writing_mode);
        if properties_that_transition.contains(physical_property) {
            continue;
        }

        properties_that_transition.insert(physical_property);
        animation_state.start_transition_if_applicable(
            context,
            &physical_property,
            transition.index,
            old_style,
            new_style,
        );
    }

    properties_that_transition
}

/// Triggers animations for a given node looking at the animation property
/// values.
pub fn maybe_start_animations<E>(
    element: E,
    context: &SharedStyleContext,
    new_style: &Arc<ComputedValues>,
    animation_state: &mut ElementAnimationSet,
    resolver: &mut StyleResolverForElement<E>,
) where
    E: TElement,
{
    let style = new_style.get_ui();
    for (i, name) in style.animation_name_iter().enumerate() {
        let name = match name.as_atom() {
            Some(atom) => atom,
            None => continue,
        };

        debug!("maybe_start_animations: name={}", name);
        let timeline = style.animation_timeline_mod(i);
        if is_none_timeline(&timeline) {
            continue;
        }
        // A progress-driven animation stands where the embedder's
        // `timeline_sample` puts it, whatever its duration.
        let progress_driven = !timeline.is_auto();
        let duration = style.animation_duration_mod(i).seconds() as f64;
        if duration == 0. && !progress_driven {
            continue;
        }

        let Some(keyframe_animation) = context.stylist.lookup_keyframes(name, element) else {
            continue;
        };

        debug!("maybe_start_animations: animation {} found", name);

        // NB: This delay may be negative, meaning that the animation may be created
        // in a state where we have advanced one or more iterations or even that the
        // animation begins in a finished state.
        //
        // A progress-driven animation takes a nominal duration, which keeps
        // its easing tolerance equal to the embedder's own sampler's, and no
        // delay: the embedder reads the specified ones from the style to place
        // it within its timeline.
        let (duration, delay) = if progress_driven {
            (1., 0.)
        } else {
            (duration, style.animation_delay_mod(i).seconds() as f64)
        };

        let iteration_count = style.animation_iteration_count_mod(i);
        let iteration_state = if iteration_count.0.is_infinite() {
            KeyframesIterationState::Infinite(0.0)
        } else {
            KeyframesIterationState::Finite(0.0, iteration_count.0 as f64)
        };

        let animation_direction = style.animation_direction_mod(i);

        let initial_direction = match animation_direction {
            AnimationDirection::Normal | AnimationDirection::Alternate => {
                AnimationDirection::Normal
            },
            AnimationDirection::Reverse | AnimationDirection::AlternateReverse => {
                AnimationDirection::Reverse
            },
        };

        let now = context.current_time_for_animations;
        let started_at = now + delay;
        let starting_progress = (now - started_at) / duration;
        let state = match (style.animation_play_state_mod(i), progress_driven) {
            (AnimationPlayState::Paused, _) => AnimationState::Paused(starting_progress),
            (AnimationPlayState::Running, false) => AnimationState::Pending,
            (AnimationPlayState::Running, true) => AnimationState::Running,
        };

        // Determine the set of animating properties. This is not equivalent to the set of changed properties
        // when one changed property overrides another. (For example, "block-size" with writing-mode: initial
        // is the same as "height")
        let mut animating_properties = PropertyDeclarationIdSet::default();
        let mut number_of_animating_properties = 0;
        for property in keyframe_animation.properties_changed.iter() {
            debug_assert!(property.is_animatable());

            if animating_properties.insert(property.to_physical(new_style.writing_mode)) {
                number_of_animating_properties += 1;
            }
        }

        let declared = DeclaredKeyframes::new(
            element,
            &keyframe_animation,
            context,
            new_style,
            style.animation_timing_function_mod(i),
            style.animation_composition_mod(i),
            resolver,
            &animating_properties,
        );

        let mut new_animation = Animation {
            name: name.clone(),
            properties_changed: keyframe_animation.properties_changed.clone(),
            keyframes: AnimationKeyframes::new(declared),
            started_at,
            duration,
            fill_mode: style.animation_fill_mode_mod(i),
            delay,
            iteration_state,
            state,
            direction: animation_direction,
            current_direction: initial_direction,
            number_of_animating_properties,
            is_new: true,
            timeline,
            timeline_sample: None,
        };

        // If we started with a negative delay, make sure we iterate the animation if
        // the delay moves us past the first iteration.
        if !progress_driven {
            new_animation.iterate_by(starting_progress);
        }

        animation_state.dirty = true;

        // If the animation was already present in the list for the node, just update its state.
        for existing_animation in animation_state.animations.iter_mut() {
            if existing_animation.state == AnimationState::Canceled {
                continue;
            }

            if new_animation.name == existing_animation.name {
                // A change between the document timeline and a progress
                // timeline starts the animation over.
                if existing_animation.is_progress_driven() != progress_driven {
                    *existing_animation = new_animation;
                } else {
                    existing_animation
                        .update_from_other(&new_animation, context.current_time_for_animations);
                }
                return;
            }
        }

        animation_state.animations.push(new_animation);
    }
}
