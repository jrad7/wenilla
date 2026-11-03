//! Which clip a body plays, by `AnimationData` id: looping, one-shots that go back to Stand, and
//! one-shots that hold their last frame. A tiny fraction of `benilla-app`'s `creature_anim`, with no
//! locomotion blending, precast, masks, variation rolls or replay counts.
//!
//! The rules are [`ClipDriver`], plain data with no Bevy in it; [`drive_clips`] applies them.

use std::time::Duration;

use benilla_assets::ModelAnimations;
use benilla_formats::AnimDataCatalog;
use bevy::animation::RepeatAnimation;
use bevy::animation::transition::AnimationTransitions;
use bevy::prelude::*;

use crate::tables::UnitTables;

/// `AnimationData.dbc`'s Stand.
pub const ANIM_STAND: u16 = 0;

/// What a clip does when it reaches its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipEnd {
    /// Starts over, until the next request.
    Loop,
    /// Goes back to a looping Stand.
    Stand,
    /// Stays on its last frame, until the next request.
    Hold,
}

/// A clip to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arm {
    /// The id the model's clip has, once the model's own substitutes are applied.
    pub id: u16,
    pub repeat: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Clip {
    id: u16,
    end: ClipEnd,
}

/// A body's clip state. Requests made while the model loads wait here, and only the latest counts.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ClipDriver {
    wanted: Option<Clip>,
    playing: Option<Clip>,
}

impl ClipDriver {
    /// Asks for `id`, replacing whatever is playing or asked for.
    pub fn play(&mut self, id: u16, end: ClipEnd) {
        self.wanted = Some(Clip { id, end });
    }

    /// One frame. `finished` says whether the playing clip has reached its end, and `resolve` turns
    /// an id into the id the model plays for it, or `None` if it has no clip for that. Returns what
    /// to start, if anything.
    pub fn step(&mut self, finished: bool, resolve: impl Fn(u16) -> Option<u16>) -> Option<Arm> {
        let next = match (self.wanted.take(), self.playing) {
            (Some(wanted), _) => wanted,
            (None, Some(playing)) if finished && playing.end == ClipEnd::Stand => Clip {
                id: ANIM_STAND,
                end: ClipEnd::Loop,
            },
            _ => return None,
        };
        let stand = resolve(ANIM_STAND);
        let clip = match resolve(next.id) {
            // A model's substitute for this id can be its idle, which is no one-shot.
            Some(id) if Some(id) != stand => Clip { id, end: next.end },
            // A missing clip falls back to Stand. A body with neither keeps what it has.
            _ => Clip {
                id: stand?,
                end: ClipEnd::Loop,
            },
        };
        // A flinch on a model with no wound clip must not jump its idle back to the start.
        if clip.end == ClipEnd::Loop && self.playing == Some(clip) {
            return None;
        }
        self.playing = Some(clip);
        Some(Arm {
            id: clip.id,
            repeat: clip.end == ClipEnd::Loop,
        })
    }

    /// The id the body plays now, once [`Self::step`] has run; `None` before its first clip.
    pub fn playing(&self) -> Option<u16> {
        self.playing.map(|clip| clip.id)
    }
}

/// The id `anims` plays for `requested`, if it has a clip for it. With no `AnimationData.dbc` an id
/// past the model's table can only match a clip of its own.
fn resolve(
    anims: &ModelAnimations,
    requested: u16,
    catalog: Option<&AnimDataCatalog>,
) -> Option<u16> {
    let id = match catalog {
        Some(catalog) => anims.resolve(requested, catalog).id,
        None => anims
            .playable_animation_lookup
            .get(usize::from(requested))
            .map_or(requested, |row| row.resolved_id),
    };
    anims.find(id).map(|clip| clip.anim_id)
}

/// Starts what each body's [`ClipDriver`] asks for, over the clip's authored cross-fade.
pub(crate) fn drive_clips(
    mut bodies: Query<(
        &mut ClipDriver,
        &ModelAnimations,
        &mut AnimationPlayer,
        &mut AnimationTransitions,
    )>,
    tables: Option<Res<UnitTables>>,
) {
    let catalog = tables
        .as_ref()
        .and_then(|tables| tables.animations.as_ref());
    for (mut driver, anims, mut player, mut transitions) in &mut bodies {
        let finished = transitions
            .get_main_animation()
            .and_then(|node| player.animation(node))
            .is_none_or(|active| active.is_finished());
        let Some(arm) = driver.step(finished, |id| resolve(anims, id, catalog)) else {
            continue;
        };
        let Some(clip) = anims.find(arm.id) else {
            continue;
        };
        let fade = Duration::from_secs_f32(clip.blend_time.max(0.0));
        transitions
            .play(&mut player, clip.node, fade)
            .set_repeat(if arm.repeat {
                RepeatAnimation::Forever
            } else {
                RepeatAnimation::Never
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEATH: u16 = 1;
    const WALK: u16 = 4;
    const WOUND: u16 = 9;
    const ATTACK: u16 = 17;
    const ATTACK_UNARMED: u16 = 16;

    /// A model with Stand, Death, Attack1H and Walk, and no wound.
    fn humanoid(id: u16) -> Option<u16> {
        matches!(id, ANIM_STAND | DEATH | ATTACK | WALK).then_some(id)
    }

    fn arm(id: u16, repeat: bool) -> Option<Arm> {
        Some(Arm { id, repeat })
    }

    #[test]
    fn a_fresh_driver_asks_for_nothing() {
        let mut driver = ClipDriver::default();
        assert_eq!(driver.step(false, humanoid), None);
        assert_eq!(driver.step(true, humanoid), None);
        assert_eq!(driver.playing(), None);
    }

    #[test]
    fn a_one_shot_plays_once_then_returns_to_a_looping_stand() {
        let mut driver = ClipDriver::default();
        driver.play(ATTACK, ClipEnd::Stand);
        assert_eq!(driver.step(false, humanoid), arm(ATTACK, false));
        assert_eq!(driver.step(false, humanoid), None);
        assert_eq!(driver.step(true, humanoid), arm(ANIM_STAND, true));
        assert_eq!(driver.playing(), Some(ANIM_STAND));
        // Stand loops: nothing more to do, finished or not.
        assert_eq!(driver.step(true, humanoid), None);
    }

    #[test]
    fn a_held_clip_stays_until_the_next_request() {
        let mut driver = ClipDriver::default();
        driver.play(DEATH, ClipEnd::Hold);
        assert_eq!(driver.step(false, humanoid), arm(DEATH, false));
        assert_eq!(driver.step(true, humanoid), None);
        assert_eq!(driver.step(true, humanoid), None);
        assert_eq!(driver.playing(), Some(DEATH));
        driver.play(ATTACK, ClipEnd::Stand);
        assert_eq!(driver.step(true, humanoid), arm(ATTACK, false));
    }

    #[test]
    fn a_loop_runs_until_the_next_request() {
        let mut driver = ClipDriver::default();
        driver.play(WALK, ClipEnd::Loop);
        assert_eq!(driver.step(false, humanoid), arm(WALK, true));
        assert_eq!(driver.step(true, humanoid), None);
        driver.play(ANIM_STAND, ClipEnd::Loop);
        assert_eq!(driver.step(false, humanoid), arm(ANIM_STAND, true));
    }

    #[test]
    fn a_missing_clip_falls_back_to_stand() {
        let mut driver = ClipDriver::default();
        driver.play(WOUND, ClipEnd::Stand);
        assert_eq!(driver.step(false, humanoid), arm(ANIM_STAND, true));
        assert_eq!(driver.playing(), Some(ANIM_STAND));
    }

    #[test]
    fn a_missing_held_clip_loops_stand_rather_than_freezing_on_it() {
        let mut driver = ClipDriver::default();
        driver.play(WOUND, ClipEnd::Hold);
        assert_eq!(driver.step(false, humanoid), arm(ANIM_STAND, true));
    }

    // Replaying the idle from its first frame would show as a twitch on every missed flinch.
    #[test]
    fn a_fallback_does_not_restart_an_idle_already_looping() {
        let mut driver = ClipDriver::default();
        driver.play(ANIM_STAND, ClipEnd::Loop);
        assert_eq!(driver.step(false, humanoid), arm(ANIM_STAND, true));
        driver.play(WOUND, ClipEnd::Stand);
        assert_eq!(driver.step(false, humanoid), None);
    }

    #[test]
    fn a_one_shot_replayed_starts_over() {
        let mut driver = ClipDriver::default();
        driver.play(ATTACK, ClipEnd::Stand);
        assert_eq!(driver.step(false, humanoid), arm(ATTACK, false));
        driver.play(ATTACK, ClipEnd::Stand);
        assert_eq!(driver.step(false, humanoid), arm(ATTACK, false));
    }

    #[test]
    fn the_models_own_substitute_is_what_plays() {
        // A beast's table sends Attack1H to its unarmed swing.
        let beast = |id| match id {
            ATTACK | ATTACK_UNARMED => Some(ATTACK_UNARMED),
            ANIM_STAND | DEATH => Some(id),
            _ => None,
        };
        let mut driver = ClipDriver::default();
        driver.play(ATTACK, ClipEnd::Stand);
        assert_eq!(driver.step(false, beast), arm(ATTACK_UNARMED, false));
        assert_eq!(driver.step(true, beast), arm(ANIM_STAND, true));
    }

    #[test]
    fn a_substitute_that_is_the_idle_loops() {
        let statue = |id| (id == ANIM_STAND || id == DEATH).then_some(ANIM_STAND);
        let mut driver = ClipDriver::default();
        driver.play(DEATH, ClipEnd::Hold);
        assert_eq!(driver.step(false, statue), arm(ANIM_STAND, true));
    }

    #[test]
    fn a_body_with_no_stand_keeps_what_it_has() {
        let only_attack = |id| (id == ATTACK).then_some(id);
        let mut driver = ClipDriver::default();
        driver.play(WOUND, ClipEnd::Stand);
        assert_eq!(driver.step(false, only_attack), None);
        driver.play(ATTACK, ClipEnd::Stand);
        assert_eq!(driver.step(false, only_attack), arm(ATTACK, false));
        // Its one-shot has nothing to return to, and does not try again every frame.
        assert_eq!(driver.step(true, only_attack), None);
        assert_eq!(driver.step(true, only_attack), None);
    }

    // The latest request wins, however long the model took to load.
    #[test]
    fn only_the_latest_request_counts() {
        let mut driver = ClipDriver::default();
        driver.play(ATTACK, ClipEnd::Stand);
        driver.play(DEATH, ClipEnd::Hold);
        assert_eq!(driver.step(false, humanoid), arm(DEATH, false));
    }
}
