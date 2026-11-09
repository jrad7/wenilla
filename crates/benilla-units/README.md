# benilla-units

Units for a program that embeds `benilla-world` with no server: a display id, or a player's race,
sex and dials, to a body standing in the world. This crate exists only on the `idle` branch of `jrad7/wenilla`. It holds no game code,
adds no edits to the crates beneath it, and leaves `benilla-app` alone, so upstream merges cannot
conflict here.

Add `UnitsPlugin` after `WorldPlugins`, then spawn `UnitBody::creature(display_id)` or
`UnitBody::character(dials)` with a `Transform` and a `Visibility`. The root's translation and rotation stay yours, and dressing sets
its scale from the display. Watch `UnitBody::state()`: `Loading`, then `Ready(BodyFacts)` or
`Failed(path)`, which spawned no part and is yours to degrade.

## What a body has

- A plain creature: its M2, `Monster1/2/3` skins from `CreatureDisplayInfo`, every geoset.
- A humanoid NPC (`CreatureDisplayInfoExtra`, a `Character\` model): the baked atlas under
  `Textures\BakedNpcTextures\`, hair and fringe from `CharSections`, geosets from its armor columns.
- A player's body (`CharacterDials`: race, sex, skin, face, hair style and colour, facial hair): the
  race's display from `ChrRaces`, at its scale, an atlas composited from `CharSections` off the main
  thread and shared by every body with that look, hair and facial geosets. It draws nothing until the
  composite lands, as the client does. An NPC with no bake name wears the same composite, with its
  armor columns painted on. A dial with no art draws without it, except a skin with no base row,
  which fails the body.
- A looping idle on the engine's rig, indoor lighting (`InteriorLit`), billboard cards following
  their joints and despawning with the body (`BodyCards`).
- Worn gear (`UnitBody::character_wearing(dials, Worn)`, or an NPC's own columns): armor painted into
  the composite and shaping the geosets, a cloak on the body's cape geoset and object batches, and the
  helm and both shoulders stood on it as riders. A helm's file is chosen by race and sex.
- Held items (`Held`: main hand, off-hand, ranged), on a character through `Worn` or on any creature
  through `UnitBody::creature_holding(display, Held)`: the main hand's in the right hand, an off-hand
  shield on the forearm and any other off-hand item in the left, a bow in the left and any other
  ranged item in the right, as the client draws them. A shield is known by its model's name
  (`held_folder`), since a display row has no inventory type. A held item stands only where the
  model authors its point. `Drawn` on the root says which show: `Melee` (the default) shows the
  pair, `Ranged` the ranged item, which 1.12 shows only while drawn. Every rider carries `WornBy`,
  so a caller can find one that failed.
- A `SeatOn { wearer, points }` hangs any body from the first attachment its wearer authors, else from
  the root, following the live bone, and goes with the wearer. Dressing waits for the seat.
- `UnitBody::prop(path, texture)` is a bare model, and `UnitBody::effect(path)` one with its particles
  and ribbons. `missile_effect` and `kit_effects` read `SpellVisual` rows to the models to stand.
- A `ClipDriver`: `play(animation_data_id, ClipEnd::Loop | Stand | Hold)`, over the clip's authored
  cross-fade. The id goes through the model's own substitutes, and one it has no clip for plays Stand.
  Requests made while the model loads wait, and the latest wins. `benilla-app`'s driver holds
  23k lines for a live unit; this is about 150, plus tests, because a program that names its clips
  needs little of what that does.

- A spell's beam (`ChainBeam`, from `benilla-app`'s `chain_beam.rs`): a kit's chain proc
  (`kit_beam`) from a caster's body through other bodies, one ribbon per hop, subdivided, jittered,
  advected and scrolled as the client draws it, in the effect lane. A cast-stage beam burns hop by
  hop and ends itself; a channel's lasts until despawned or its caster goes. A hop whose body is
  not dressed or not shown is not drawn.

## What it does not have yet

Compared with `benilla-app`'s `entities`:

- On a worn body: ammo or a quiver; sheathing, since a held item is drawn or hidden, never stowed on
  the back or hip (that needs the item's sheath type); an NPC's cloak, which `CreatureDisplayInfoExtra`
  has no column for; item glows and enchant trails; a worn item's own particles. Worn gear is fixed at spawn, so changing it means a new body. Guild
  emblems, the composite's forced lane for previews and its distance sweep: the atlases stay cached
  for the process's life.
- On an effect: ground decals, sounds, launch kits, a thrown weapon's in-flight clip. (Stages came
  with `EffectLife`.)
- On a beam (`ChainBeam`): the caster's `$CSL` marker (its end is its right hand, else its left, else
  the client's fallback of three quarters of the model's height), and a channel's re-targeting; a beam runs through the bodies it
  was given until it ends.
- Clip choice of its own: gait from speed, locomotion blending, precast, upper-body masks, sheathing,
  variation rolls and replay counts (the head variation plays, once).
- Particle emitters, ribbons and carried lights.
- Appear fades, `WorldUnit` (so no water ripples, and a body is never culled by a sealed room), and
  per-instance animated materials (`mark_mat_anim`).
- A model file the client does not ship stands the engine's error cube rather than failing, as in
  the client.
