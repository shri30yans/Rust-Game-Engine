# IMAGINE: look

The art direction of IMAGINE's 3D world. The renderer that draws it, its tiers and its test scenes are platform docs: [docs/RENDERING.md](../../../docs/RENDERING.md). The UI look is in [design/](design/README.md).

## 1. Art direction (the world)

- **Setting:** a near-future city, worn but lived in. Each season gives it a theme (launch: a city in ruins; the construction site Kiln Yard is the first map).
- **Style:** stylised realism. Real-world proportions, physically based materials, clean shapes that read at a distance. Characters and guns are simpler than the world: they are placeholders for now ([ASSETS.md](ASSETS.md) §2).
- **Colour:** neutral concrete, steel and brick dominate (albedo 0.35–0.5 linear, roughness 0.7–0.9). Arena colours stay lower in saturation than character accents. Saturated colour otherwise comes from **lights, signs, team colours and effects**, and fills under 10 % of any frame, so the things that matter stand out.
- **Light:** warm sodium and tungsten work lights (2700 K) against a cool sky fill (7500 K). Outdoors the exposure is around EV 14; indoors EV 9–10, reached through auto exposure.
- **Grade:** AgX-style filmic base, +8 % saturation, slightly teal shadows and warm highlights, vignette 0.2. One grade per season, stored as a colour lookup table.
- **Destruction look (D52):** staged like the reference ([REFERENCE.md](REFERENCE.md) §1a): big pieces fall in 1–2 s, dust rolls out and hangs 6–12 s, rubble stays for the round. Broken faces show inner material (brick, rebar, insulation). Explosions are layered (flash, white-hot fireball, rolling smoke, spark streaks, debris, light, scorch, shake). Each area of an arena has one dominant colour you can name.
- **Arena changes look:** every Director change is staged: warning lights and sirens, then visible machinery (pistons, rails, steam) as walls and bridges move. Small breakables (glass, crates) shatter with lit dust.
- **Readability rules:** team colour covers at least 8 % of a character's silhouette (shoulders and back panel); enemies are never darker than the background they stand on (rim light in shadow). Character chest and head accents are the most saturated colours on screen: warm for enemies and cool for allies.

