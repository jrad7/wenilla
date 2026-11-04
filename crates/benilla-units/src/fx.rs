//! `SpellVisual` rows to the effect models a caller stands as [`crate::UnitBody::effect`].

use benilla_formats::SpellVisualCatalog;

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

#[cfg(test)]
mod tests {
    use benilla_formats::load_spell_visual_catalog;

    use super::*;

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
