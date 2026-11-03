//! The client tables a body is built from.

use benilla_assets::WorldAssets;
use benilla_formats::{
    AnimDataCatalog, Chain, CharCreateCatalog, CharSections, CharacterGeosets, CreatureCatalog,
    ItemDisplayCatalog, load_anim_data_catalog, load_creature_catalog, load_item_display_catalog,
};
use bevy::prelude::*;

/// The tables, read once. Only the creature catalog is essential: without the others a humanoid
/// draws every geoset, or an untextured hair, rather than nothing, and a character body fails.
#[derive(Resource)]
pub struct UnitTables {
    pub creatures: CreatureCatalog,
    pub geosets: Option<CharacterGeosets>,
    pub sections: Option<CharSections>,
    pub items: Option<ItemDisplayCatalog>,
    pub animations: Option<AnimDataCatalog>,
    /// Race and sex to a body display.
    pub create: Option<CharCreateCatalog>,
}

impl UnitTables {
    pub fn load(chain: &mut Chain) -> anyhow::Result<Self> {
        Ok(Self {
            creatures: load_creature_catalog(chain)?,
            geosets: optional("character geosets", CharacterGeosets::load(chain)),
            sections: optional("char sections", CharSections::load(chain)),
            items: optional("item displays", load_item_display_catalog(chain)),
            animations: optional("animation data", load_anim_data_catalog(chain)),
            create: optional("character creation", CharCreateCatalog::load(chain)),
        })
    }
}

fn optional<T>(what: &str, table: anyhow::Result<T>) -> Option<T> {
    table
        .map_err(|error| warn!("{what} unavailable: {error:#}"))
        .ok()
}

/// With no client there is no chain and no tables, and every body fails, which is its caller's to
/// degrade.
pub(crate) fn load_tables(mut commands: Commands, assets: Option<Res<WorldAssets>>) {
    let Some(assets) = assets else {
        return;
    };
    let mut chain = assets
        .chain
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match UnitTables::load(&mut chain) {
        Ok(tables) => commands.insert_resource(tables),
        Err(error) => warn!("creature tables unavailable: {error:#}"),
    }
}
