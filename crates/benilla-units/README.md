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
- A `ClipDriver`: `play(animation_data_id, ClipEnd::Loop | Stand | Hold)`, over the clip's authored
  cross-fade. The id goes through the model's own substitutes, and one it has no clip for plays Stand.
  Requests made while the model loads wait, and the latest wins. `benilla-app`'s driver holds
  23k lines for a live unit; this is about 150, plus tests, because a program that names its clips
  needs little of what that does.

## What it does not have yet

Compared with `benilla-app`'s `entities`:

- A player's worn armor: `CharacterDials` has no equipment yet. Guild emblems, the composite's forced
  lane for previews and its distance sweep: the atlases stay cached for the process's life.
- Helm and shoulder models, and held weapons. NPC bakes include their armor, but those models attach.
- Clip choice of its own: gait from speed, locomotion blending, precast, upper-body masks, sheathing,
  variation rolls and replay counts (the head variation plays, once).
- Particle emitters, ribbons and carried lights.
- Appear fades, `WorldUnit` (so no water ripples, and a body is never culled by a sealed room), and
  per-instance animated materials (`mark_mat_anim`).
- A model file the client does not ship stands the engine's error cube rather than failing, as in
  the client.
