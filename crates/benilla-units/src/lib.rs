//! Units for an embedding program: a display id to a body standing in `benilla-world`, with no
//! game, network or UI attached. Nothing here knows who is asking.
//!
//! The client's own unit code lives in `benilla-app`, private and tied to its network entities.
//! This crate is the small part of it a program with no server needs, written against the engine's
//! public API. It edits nothing upstream.
//!
//! Add [`UnitsPlugin`] after `WorldPlugins`, spawn [`UnitBody`] with a `Transform` and a
//! `Visibility`, and watch its [`BodyState`].

mod body;
mod look;
mod tables;

use benilla_assets::AssetSet;
use bevy::prelude::*;

pub use body::{BodyCards, BodyFacts, BodyState, CardOf, UnitBody, attachment_point};
pub use look::{CreatureLook, PartSkin, creature_look, is_character_model, model_dir, part_skin};
pub use tables::UnitTables;

/// Where bodies are dressed each frame, for a caller to order itself against.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnitsSet;

/// Loads the display tables at startup and dresses every [`UnitBody`] as its files arrive.
pub struct UnitsPlugin;

impl Plugin for UnitsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, tables::load_tables.after(AssetSet::Open))
            .add_systems(Update, body::dress_bodies.in_set(UnitsSet));
    }
}
