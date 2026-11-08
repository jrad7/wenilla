//! `SpellVisual` rows to the effect models a caller stands as [`crate::UnitBody::effect`], and how
//! such a model lives once it stands: [`EffectLife`].
//!
//! The lifecycle is `benilla-app`'s `spell_fx/lifecycle.rs` (birth, `Hold`, `Decay`) on a bare body,
//! with its span clock folded in: a one-shot ends at its birth's first completion rather than on a
//! clock set from the length of the file's first sequence, the same moment for a `Spells\` model
//! whose first sequence is its `Stand`.

use benilla_assets::ModelAnimations;
use benilla_formats::SpellVisualCatalog;
use bevy::animation::RepeatAnimation;
use bevy::animation::graph::AnimationNodeIndex;
use bevy::prelude::*;

use crate::body::{BodyState, UnitBody};

/// `AnimationData.dbc` 158 `Hold`, the sustained pulse a state effect settles into.
pub const ANIM_HOLD: u16 = 158;
/// `AnimationData.dbc` 159 `Decay`, the fade-out a reaped effect plays before it goes.
pub const ANIM_DECAY: u16 = 159;
/// How long a one-shot with no clips stands: `benilla-app`'s `FALLBACK_SPAN`.
const FALLBACK_SPAN: f32 = 1.0;

/// What a model lacking a kit's attachment hangs from instead: `CMissile`'s cascade, chest then base,
/// and after those the root.
pub const ATTACH_FALLBACKS: [u16; 2] = [0xf, 0x13];

/// The model a visual's missile flies; `None` for a visual with no missile of its own.
pub fn missile_effect(catalog: &SpellVisualCatalog, visual: u32) -> Option<String> {
    let stages = catalog.stages(visual)?;
    catalog.effect_path(stages.missile_model).map(str::to_owned)
}

/// A kit's `(attachment id, model)` pairs. A kit id of 0, or one with no models, is empty.
pub fn kit_effects(catalog: &SpellVisualCatalog, kit: u32) -> Vec<(u16, String)> {
    catalog.kit(kit).map_or_else(Vec::new, |kit| {
        kit.effects()
            .filter_map(|(tag, effect)| catalog.effect_path(effect).map(|p| (tag, p.to_owned())))
            .collect()
    })
}

/// Which kit stood an effect, and so how it lives: `benilla-app`'s `FxStage`, as the client's
/// `PlaySpellVisualKit` (`0x60edf0`) picks a completion callback per kit stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectStage {
    /// A cast's flash or an impact, kit stages 0 and 1: one pass of its birth, then gone
    /// (`0x5fbf50`).
    OneShot,
    /// An aura's state or a channel, stage 2: its birth, then `Hold` if the model authors one, else
    /// parked on the birth (`0x5ff170`), until reaped.
    State,
    /// A precast, stages 3 and 4: its birth over and over (`0x60ed00`), until reaped.
    Relive,
}

/// Put beside [`UnitBody::effect`]: the effect then plays its stage's legs and despawns itself when
/// it is over, a one-shot after one pass and anything after [`EffectLife::reap`] and its `Decay`, if
/// its model authors one (`0x614150`). A body that fails is left for its owner to see and remove.
#[derive(Component, Clone, Debug)]
pub struct EffectLife {
    stage: EffectStage,
    leg: Leg,
    reaped: bool,
    /// Seconds stood with no clip to time it by.
    bare_secs: f32,
}

/// Where an effect's model is in its lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leg {
    /// Not dressed yet, or dressed this frame.
    Unarmed,
    /// Playing its birth, watching for the first completion.
    Birth(AnimationNodeIndex),
    /// Nothing changes until a reap: a precast's repeat, a state's `Hold` or its parked birth.
    Settled(AnimationNodeIndex),
    /// Reaped and playing `Decay` out.
    Decay(AnimationNodeIndex),
    /// A model with no clip.
    Bare,
}

/// What [`live_effects`] does to an effect this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Wait,
    Despawn,
    Hold,
    Park,
    Decay,
}

impl EffectLife {
    pub fn new(stage: EffectStage) -> Self {
        Self {
            stage,
            leg: Leg::Unarmed,
            reaped: false,
            bare_secs: 0.0,
        }
    }

    pub fn stage(&self) -> EffectStage {
        self.stage
    }

    /// Ends it: `Decay` played out if the model authors one, else gone at once. A second reap
    /// changes nothing.
    pub fn reap(&mut self) {
        self.reaped = true;
    }

    pub fn reaped(&self) -> bool {
        self.reaped
    }

    /// The lifecycle's rule, with no Bevy in it. `first_pass` is whether the armed clip has completed
    /// a pass; `has_hold` and `has_decay` whether the model authors those clips.
    fn step(&self, first_pass: bool, has_hold: bool, has_decay: bool) -> Step {
        match self.leg {
            Leg::Unarmed => Step::Wait,
            Leg::Decay(_) if first_pass => Step::Despawn,
            Leg::Decay(_) => Step::Wait,
            Leg::Bare => {
                let over = self.stage == EffectStage::OneShot && self.bare_secs >= FALLBACK_SPAN;
                if self.reaped || over {
                    Step::Despawn
                } else {
                    Step::Wait
                }
            }
            Leg::Birth(_) | Leg::Settled(_) if self.reaped => {
                if has_decay {
                    Step::Decay
                } else {
                    Step::Despawn
                }
            }
            Leg::Settled(_) => Step::Wait,
            Leg::Birth(_) if !first_pass => Step::Wait,
            Leg::Birth(_) => match self.stage {
                EffectStage::OneShot => Step::Despawn,
                EffectStage::State if has_hold => Step::Hold,
                EffectStage::State | EffectStage::Relive => Step::Park,
            },
        }
    }
}

/// Arms a dressed effect's birth: `Stand`, as the client's model-load bootstrap does, not the
/// file's first sequence. A precast repeats whatever the sequence flag says (`0x60ed00` reads none);
/// the rest wrap a looping sequence and clamp one that is not, as the M2 sampler does.
fn arm_birth(player: &mut AnimationPlayer, anims: &ModelAnimations, stage: EffectStage) -> Leg {
    let Some(clip) = anims.preferred_clip(None) else {
        return Leg::Bare;
    };
    player.stop_all();
    let repeat = if clip.looping || stage == EffectStage::Relive {
        RepeatAnimation::Forever
    } else {
        RepeatAnimation::Never
    };
    player.start(clip.node).set_repeat(repeat);
    match stage {
        EffectStage::Relive => Leg::Settled(clip.node),
        EffectStage::OneShot | EffectStage::State => Leg::Birth(clip.node),
    }
}

/// What [`live_effects`] reads of each effect.
type Effects<'a> = (
    Entity,
    &'a UnitBody,
    &'a mut EffectLife,
    Option<&'a ModelAnimations>,
    Option<&'a mut AnimationPlayer>,
);

/// Runs each [`EffectLife`]: arms the birth once its body is dressed, hands a state over to `Hold`,
/// plays `Decay` on a reap and despawns what is over. Completion is a first pass, not
/// `is_finished`, since a wrapping birth never finishes.
pub(crate) fn live_effects(mut commands: Commands, time: Res<Time>, mut effects: Query<Effects>) {
    let dt = time.delta_secs();
    for (entity, body, mut life, anims, mut player) in &mut effects {
        match body.state() {
            BodyState::Failed(_) => continue,
            BodyState::Loading => {
                // Nothing on screen yet to fade.
                if life.reaped {
                    commands.entity(entity).despawn();
                }
                continue;
            }
            BodyState::Ready(_) => {}
        }
        if life.leg == Leg::Unarmed {
            match (anims, player.as_deref_mut()) {
                (Some(anims), Some(player)) => {
                    let stage = life.stage;
                    life.leg = arm_birth(player, anims, stage);
                }
                (None, _) => life.leg = Leg::Bare,
                // The player comes with the dressing's commands.
                (Some(_), None) => continue,
            }
        }
        if life.leg == Leg::Bare {
            life.bare_secs += dt;
        }
        let armed = match life.leg {
            Leg::Birth(node) | Leg::Settled(node) | Leg::Decay(node) => Some(node),
            Leg::Unarmed | Leg::Bare => None,
        };
        let first_pass = armed
            .zip(player.as_deref())
            .and_then(|(node, player)| player.animation(node))
            .is_none_or(|active| active.completions() >= 1 || active.is_finished());
        let find = |id| anims.and_then(|anims| anims.find(id));
        match life.step(
            first_pass,
            find(ANIM_HOLD).is_some(),
            find(ANIM_DECAY).is_some(),
        ) {
            Step::Wait => {}
            Step::Despawn => commands.entity(entity).despawn(),
            Step::Park => {
                if let Some(node) = armed {
                    life.leg = Leg::Settled(node);
                }
            }
            Step::Hold | Step::Decay => {
                let decay = life.reaped;
                let next = find(if decay { ANIM_DECAY } else { ANIM_HOLD });
                let (Some(player), Some(clip), Some(node)) = (player.as_deref_mut(), next, armed)
                else {
                    continue;
                };
                player.stop(node);
                let play = player.start(clip.node);
                if decay {
                    // Never repeated: the effect goes at this sequence's end.
                    play.set_repeat(RepeatAnimation::Never);
                    life.leg = Leg::Decay(clip.node);
                } else {
                    play.set_repeat(RepeatAnimation::Forever);
                    life.leg = Leg::Settled(clip.node);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use benilla_formats::load_spell_visual_catalog;

    use super::*;

    fn node() -> AnimationNodeIndex {
        AnimationNodeIndex::new(0)
    }

    fn armed(stage: EffectStage, leg: Leg) -> EffectLife {
        EffectLife {
            leg,
            ..EffectLife::new(stage)
        }
    }

    // An impact or a cast's flash goes at its birth's first completion, and not before.
    #[test]
    fn a_one_shot_goes_after_one_pass() {
        let life = armed(EffectStage::OneShot, Leg::Birth(node()));
        assert_eq!(life.step(false, true, true), Step::Wait);
        assert_eq!(life.step(true, true, true), Step::Despawn);
    }

    // `0x5ff170`: a state's birth hands over to `Hold` if the model authors one, else parks.
    #[test]
    fn a_state_holds_or_parks_after_its_birth() {
        let life = armed(EffectStage::State, Leg::Birth(node()));
        assert_eq!(life.step(true, true, false), Step::Hold);
        assert_eq!(life.step(true, false, false), Step::Park);
        let settled = armed(EffectStage::State, Leg::Settled(node()));
        assert_eq!(settled.step(true, true, true), Step::Wait);
    }

    // A precast repeats until reaped, however many passes it makes.
    #[test]
    fn a_precast_waits_for_its_reap() {
        let mut life = armed(EffectStage::Relive, Leg::Settled(node()));
        assert_eq!(life.step(true, true, true), Step::Wait);
        life.reap();
        assert_eq!(life.step(true, true, true), Step::Decay);
    }

    // `0x614150`: a reap plays `Decay` out if authored, else ends at once; `Decay`'s end ends it.
    #[test]
    fn a_reap_decays_or_ends_and_decay_ends_it() {
        let mut life = armed(EffectStage::State, Leg::Settled(node()));
        life.reap();
        assert_eq!(life.step(false, true, true), Step::Decay);
        assert_eq!(life.step(false, true, false), Step::Despawn);
        let decaying = EffectLife {
            reaped: true,
            ..armed(EffectStage::State, Leg::Decay(node()))
        };
        assert_eq!(decaying.step(false, true, true), Step::Wait);
        assert_eq!(decaying.step(true, true, true), Step::Despawn);
    }

    // A model with no clip: a one-shot stands `FALLBACK_SPAN`, anything else until its reap.
    #[test]
    fn a_bare_model_goes_on_the_fallback_span_or_a_reap() {
        let mut life = armed(EffectStage::OneShot, Leg::Bare);
        assert_eq!(life.step(true, false, false), Step::Wait);
        life.bare_secs = FALLBACK_SPAN;
        assert_eq!(life.step(true, false, false), Step::Despawn);
        let mut aura = armed(EffectStage::State, Leg::Bare);
        aura.bare_secs = 60.0;
        assert_eq!(aura.step(true, false, false), Step::Wait);
        aura.reap();
        assert_eq!(aura.step(true, false, false), Step::Despawn);
    }

    // Nothing happens before the body is dressed and armed.
    #[test]
    fn an_unarmed_effect_waits() {
        let mut life = EffectLife::new(EffectStage::OneShot);
        life.reap();
        assert_eq!(life.step(true, true, true), Step::Wait);
    }

    // Against the client's own tables, skipped without one: the probe's visuals are what they were
    // picked to be, and their files are there.
    #[test]
    fn the_probes_visuals_name_real_files_in_a_real_client() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let catalog = load_spell_visual_catalog(&mut chain).expect("visuals");
        let missile = missile_effect(&catalog, 28).expect("a missile");
        assert!(missile.contains("FireBolt"), "{missile}");
        let impact = catalog.stages(28).expect("a row").impact;
        let renew = catalog.stages(280).expect("a row").state;
        for (_, path) in [
            kit_effects(&catalog, impact),
            kit_effects(&catalog, renew),
            vec![(0, missile)],
        ]
        .concat()
        {
            let m2 = path.replace(".mdx", ".m2").replace(".mdl", ".m2");
            println!("{m2}");
            assert!(chain.read_file(&m2).is_ok(), "{m2}");
        }
        assert!(kit_effects(&catalog, 0).is_empty());
        assert_eq!(missile_effect(&catalog, 0), None);
    }
}
