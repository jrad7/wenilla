# benilla-units

Units for a program that embeds `benilla-world` with no server: a display id to a body standing in
the world. This crate exists only on the `idle` branch of `jrad7/wenilla`. It holds no game code,
adds no edits to the crates beneath it, and leaves `benilla-app` alone, so upstream merges cannot
conflict here.

Add `UnitsPlugin` after `WorldPlugins`, then spawn `UnitBody::creature(display_id)` with a
`Transform` and a `Visibility`. The root's translation and rotation stay yours, and dressing sets
its scale from the display. Watch `UnitBody::state()`: `Loading`, then `Ready(BodyFacts)` or
`Failed(path)`, which spawned no part and is yours to degrade.

## What a body has

- A plain creature: its M2, `Monster1/2/3` skins from `CreatureDisplayInfo`, every geoset.
- A humanoid NPC (`CreatureDisplayInfoExtra`, a `Character\` model): the baked atlas under
  `Textures\BakedNpcTextures\`, hair and fringe from `CharSections`, geosets from its armor columns.
- A looping idle on the engine's rig, indoor lighting (`InteriorLit`), billboard cards following
  their joints and despawning with the body (`BodyCards`).

## What it does not have yet

Compared with `benilla-app`'s `entities`:

- A humanoid with no bake name draws its fallback body, as the live composite is not here.
- Helm and shoulder models, and held weapons. NPC bakes include their armor, but those models attach.
- Clip choice beyond the idle.
- Particle emitters, ribbons and carried lights.
- Appear fades, `WorldUnit` (so no water ripples, and a body is never culled by a sealed room), and
  per-instance animated materials (`mark_mat_anim`).
- A model file the client does not ship stands the engine's error cube rather than failing, as in
  the client.
