//! Units for an embedding program: a display id to a body standing in `benilla-world`, with no
//! game, network or UI attached. Nothing here knows who is asking.
//!
//! The client's own unit code lives in `benilla-app`, private and tied to its network entities.
//! This crate is the small part of it a program with no server needs, written against the engine's
//! public API. It edits nothing upstream.
//!
//! Add [`UnitsPlugin`] after `WorldPlugins`, spawn [`UnitBody`] with a `Transform` and a
//! `Visibility`, and watch its [`BodyState`]. Its [`ClipDriver`] takes `AnimationData` ids. A body is
//! a creature display or a player's race, sex and dials, or a bare model, which is how a worn item
//! and a spell's effect stand; [`SeatOn`] hangs one from a body's attachment. A [`ChainBeam`] runs a
//! spell's beam from one body through others.

mod beam;
mod body;
mod clips;
mod composite;
mod fx;
mod gear;
mod look;
mod seat;
mod tables;

use benilla_assets::AssetSet;
use bevy::prelude::*;

pub use beam::{CHAIN_ATTACH_FALLBACK, ChainBeam, beam_attachment, kit_beam};
pub use body::{BodyCards, BodyFacts, BodyState, CardOf, UnitBody, attachment_point};
pub use clips::{ANIM_STAND, Arm, ClipDriver, ClipEnd};
pub use fx::{
    ANIM_DECAY, ANIM_HOLD, ATTACH_FALLBACKS, EffectLife, EffectStage, kit_effects, missile_effect,
};
pub use gear::{
    ATTACH_HAND_LEFT, ATTACH_HAND_RIGHT, ATTACH_SHIELD, Cape, Drawn, Held, Worn, WornBy, WornSlot,
    held_folder,
};
pub use look::{
    CharacterDials, CreatureLook, PartSkin, character_look, creature_look, dressed_look,
    is_character_model, model_dir, part_skin,
};
pub use seat::SeatOn;
pub use tables::UnitTables;

/// Where bodies are dressed and their clips started each frame, for a caller to order itself against.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnitsSet;

/// Loads the display tables at startup, composites body skins off the main thread, dresses every
/// [`UnitBody`] as its files arrive, and starts the clips its [`ClipDriver`] asks for.
pub struct UnitsPlugin;

impl Plugin for UnitsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<composite::SkinComposites>()
            .add_systems(Startup, tables::load_tables.after(AssetSet::Open))
            .add_systems(
                Update,
                (
                    composite::land_skin_composites,
                    seat::seat_riders,
                    body::dress_bodies,
                    gear::show_drawn,
                    clips::drive_clips,
                    fx::live_effects,
                )
                    .chain()
                    .in_set(UnitsSet),
            )
            // A beam's ends are attachment points at the pose the rig finalizer just wrote, and the
            // effect stream is cleared by `begin_effect_frame`, so it draws after both.
            .add_systems(
                PostUpdate,
                beam::draw_beams
                    .in_set(benilla_world::billboard::BillboardPlace)
                    .after(benilla_world::billboard::billboard_joint_palette)
                    .after(benilla_world::rig_anim::finalize_rig_worlds)
                    .after(benilla_world::particles::buffer::begin_effect_frame),
            );
    }
}
