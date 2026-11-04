//! What a humanoid wears, as plain data: which item models stand on it, and where. The armor's
//! textures and geosets need nothing here, since the composite and the geoset table read the same
//! columns. Taken from the path and attachment rules of `benilla-app`'s `entities/equipment`.

use benilla_formats::{ChrRaces, ItemDisplayCatalog, NpcAppearance};

/// `M2` attachment ids a worn item hangs from.
pub const ATTACH_HAND_RIGHT: u16 = 1;
pub const ATTACH_SHOULDER_RIGHT: u16 = 5;
pub const ATTACH_SHOULDER_LEFT: u16 = 6;
pub const ATTACH_HELM: u16 = 11;

/// Where an appearance's columns hold the helm and the shoulders.
const COLUMN_HELM: usize = 0;
const COLUMN_SHOULDER: usize = 1;

/// `ItemDisplayInfo` ids on a body, 0 for nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Worn {
    /// In body-slot order: head, shoulder, shirt, chest, belt, pants, boots, wrist, gloves, tabard.
    /// There is no cloak column, as in `CreatureDisplayInfoExtra`.
    pub equipment: [u32; 10],
    pub main_hand: u32,
}

/// One item model to stand on a body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rider {
    pub path: String,
    pub texture: Option<String>,
    pub attach: u16,
}

fn rider(dir: &str, name: String, texture: Option<&String>, attach: u16) -> Rider {
    Rider {
        path: format!("Item\\ObjectComponents\\{dir}\\{name}"),
        texture: texture.cloned(),
        attach,
    }
}

/// `<stem>_<prefix><M|F>.m2`: a helm has a model per race and sex. `None` for a race with no prefix.
fn helm_file(model: &str, races: &ChrRaces, race: u8, sex: u8) -> Option<String> {
    let prefix = races.client_prefix(u32::from(race))?;
    let letter = if sex == 1 { 'F' } else { 'M' };
    let stem = model.strip_suffix(".m2").unwrap_or(model);
    Some(format!("{stem}_{prefix}{letter}.m2"))
}

/// Every item model that stands on this appearance. A row naming no model adds nothing: a helm can be
/// only a hide mask, and most armor is only textures.
pub(crate) fn npc_riders(
    npc: &NpcAppearance,
    main_hand: u32,
    items: Option<&ItemDisplayCatalog>,
    races: Option<&ChrRaces>,
) -> Vec<Rider> {
    let Some(items) = items else {
        return Vec::new();
    };
    let mut riders = Vec::new();
    if let Some(row) = items.get(npc.equipment[COLUMN_HELM])
        && let (Some(model), Some(races)) = (row.model[0].as_deref(), races)
        && let Some(file) = helm_file(model, races, npc.race, npc.sex)
    {
        riders.push(rider(
            "Head",
            file,
            row.model_texture[0].as_ref(),
            ATTACH_HELM,
        ));
    }
    if let Some(row) = items.get(npc.equipment[COLUMN_SHOULDER]) {
        // The display's first column is the left shoulder.
        for (column, attach) in [ATTACH_SHOULDER_LEFT, ATTACH_SHOULDER_RIGHT]
            .into_iter()
            .enumerate()
        {
            if let Some(model) = row.model[column].clone() {
                riders.push(rider(
                    "Shoulder",
                    model,
                    row.model_texture[column].as_ref(),
                    attach,
                ));
            }
        }
    }
    if let Some(row) = items.get(main_hand)
        && let Some(model) = row.model[0].clone()
    {
        riders.push(rider(
            "Weapon",
            model,
            row.model_texture[0].as_ref(),
            ATTACH_HAND_RIGHT,
        ));
    }
    riders
}

#[cfg(test)]
mod tests {
    use benilla_formats::{load_chr_races, load_item_display_catalog};

    use super::*;

    fn wearing(race: u8, sex: u8, helm: u32, shoulder: u32) -> NpcAppearance {
        let mut equipment = [0; 10];
        equipment[COLUMN_HELM] = helm;
        equipment[COLUMN_SHOULDER] = shoulder;
        NpcAppearance {
            race,
            sex,
            skin: 0,
            face: 0,
            hair_style: 0,
            hair_color: 0,
            facial_hair: 0,
            equipment,
            bake_name: None,
        }
    }

    #[test]
    fn with_no_item_table_nothing_stands_on_a_body() {
        assert!(npc_riders(&wearing(1, 0, 7, 7), 7, None, None).is_empty());
    }

    // Against the client's own tables, skipped without one: the probe's helm, shoulders and sword
    // name real files, for two races' helms.
    #[test]
    fn worn_items_name_real_files_in_a_real_client() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let items = load_item_display_catalog(&mut chain).expect("items");
        let races = load_chr_races(&mut chain).expect("races");
        for (race, sex) in [(1, 0), (6, 1)] {
            let riders = npc_riders(
                &wearing(race, sex, 1128, 1068),
                862,
                Some(&items),
                Some(&races),
            );
            let points: Vec<u16> = riders.iter().map(|r| r.attach).collect();
            assert_eq!(points, [11, 6, 5, 1], "{riders:?}");
            for rider in &riders {
                println!("{rider:?}");
                assert!(chain.read_file(&rider.path).is_ok(), "{}", rider.path);
                let dir = rider.path.rsplit_once('\\').expect("a dir").0;
                let texture = format!("{dir}\\{}.blp", rider.texture.as_ref().expect("a skin"));
                assert!(chain.read_file(&texture).is_ok(), "{texture}");
            }
        }
    }
}
