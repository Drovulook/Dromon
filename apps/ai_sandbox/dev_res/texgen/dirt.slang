// Recette : terre brune isotrope, gravier épars, grains de sable, rares touches de mousse.
// Contrat : procgen/lib/texgen.slang.
#include "procgen/texgen.slang"
#include "lib/ground.slang"

Surface surface(float2 p, uint seed) {
  // --- Sol ---
  float soilH = 0.0020 * fbmP(p, 6, 5, seed) + 0.0004 * fbmP(p, 96, 3, seed + 1u);
  // Pas de taches à l'échelle de la tuile (répétition visible de loin) : variations
  // de couleur à partir de ~1/12 de tuile.
  float mottle = fbmP(p, 12, 4, seed + 2u);      // zones claires / sombres
  float hue = fbmP(p, 16, 3, seed + 3u);         // brun chaud ↔ gris-brun
  float grain = fbmP(p, 128, 3, seed + 4u);      // grain fin
  float crust = fbmP(p, 24, 4, seed + 5u);       // croûte sèche plus claire

  // Grain motteux : bruit « turbulent » (|bruit|) → sillons fins et irréguliers.
  float crumb = abs(gradientNoiseP(p, 160, seed + 6u))
                + 0.5 * abs(gradientNoiseP(p, 330, seed + 7u));
  soilH += 0.0007 * crumb;

  float3 earth = lerp(float3(0.205, 0.14, 0.078), float3(0.195, 0.14, 0.086),
                      smoothstep(-0.3, 0.3, hue));
  earth *= (1.0 + 0.18 * mottle) * (1.0 + 0.25 * grain);
  earth *= 0.75 + 0.35 * crumb;
  earth = lerp(earth, float3(0.24, 0.185, 0.125), 0.15 * smoothstep(-0.1, 0.4, crust));

  // --- Grains de sable (période 300 → ~2 px) ---
  Stones grit = scatterStones(p, 300, 0.35, float2(0.15, 0.35), 0.5, 0.1, seed + 10u);
  // --- Gravillons ---
  Stones pebbles = scatterStones(p, 90, 0.16, float2(0.15, 0.45), 0.6, 0.18, seed + 20u);
  // --- Pierres ---
  Stones stones = scatterStones(p, 36, 0.10, float2(0.12, 0.40), 0.6, 0.22, seed + 30u);

  // Grains à moitié fondus dans la terre (poussière).
  float3 albedo = lerp(earth, lerp(stoneColor(grit.id), earth, 0.4), 0.7 * grit.cover);
  float height = soilH + grit.height;

  // --- Mousse : petites touches dans quelques régions ---
  float region = smoothstep(0.0, 0.35, fbmP(p, 10, 3, seed + 40u));
  float fuzz = fbmP(p, 256, 2, seed + 41u);
  float clump = fbmP(p, 28, 4, seed + 42u) + 0.12 * fuzz + 0.15 * region;
  float moss = 0.9 * smoothstep(0.42, 0.50, clump);
  float3 mossColor = lerp(float3(0.10, 0.13, 0.03), float3(0.24, 0.32, 0.06),
                          saturate(0.5 + fuzz + 0.3 * fbmP(p, 64, 2, seed + 43u)));
  albedo = lerp(albedo, mossColor, moss);
  height += 0.0008 * moss * (1.0 + 0.6 * fuzz);

  // Pierres posées par-dessus (la plus haute couvre).
  Stones top = pebbles.height > stones.height ? pebbles : stones;
  float speckle = gradientNoiseP(p, 400, seed + 50u);
  float3 rock = stoneColor(top.id) * (1.0 + 0.25 * speckle)
                * (1.0 + 0.15 * top.local.x); // nuance sur la pierre, pas un éclairage
  // Terre collée sur la pierre : contraste atténué, plus sur les bords.
  // Les grosses pierres, repérables de loin, sont plus fondues que les gravillons.
  float big = pebbles.height > stones.height ? 0.0 : 0.2;
  float dust = 0.25 + big + 0.25 * saturate(dot(top.local, top.local)) + 0.2 * crust;
  rock = lerp(rock, earth, saturate(dust));
  albedo = lerp(albedo, rock, top.cover);
  height = max(height, soilH + top.height);

  Surface s = { height, saturate(albedo) };
  return s;
}
