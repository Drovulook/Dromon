// Recette : terre brune isotrope, gravier épars, grains de sable, rares touches de mousse.
// Contrat : procgen/lib/texgen.slang.
#include "procgen/texgen.slang"
#include "lib/ground.slang"
#include "lib/detail.slang"
#include "lib/scale.slang"

Surface surface(float2 p, uint seed) {
  // --- Sol ---
  // Mottes (25 → 5 cm) et rugosité à arêtes vives (9 → 2 cm) : le détail lu de près.
  // `r` : coordonnées déjà tournées (période ÷√5) : la 1ʳᵉ octave d'un bruit fin, qui
  // domine, serait sinon alignée sur les axes → trame de tissage.
  float2 r = nextOctave(p);
  float clods = billowR(r, 18, 3, 0.55, seed + 8u);
  float rough = ridgedR(r, 49, 3, 0.6, seed + 9u);
  float soilH = cm(4.0) * fbmP(p, 6, 3, seed) + cm(3.0) * clods + cm(0.9) * rough;
  // Pas de taches à l'échelle de la tuile (répétition visible de loin) : variations
  // de couleur à partir de ~1/12 de tuile.
  float mottle = fbmR(p, 12, 4, 0.5, seed + 2u);    // zones claires / sombres
  float hue = fbmR(p, 16, 3, 0.5, seed + 3u);       // brun chaud ↔ gris-brun
  float grain = fbmR(r, 57, 3, 0.55, seed + 4u);    // grain fin
  float crust = fbmR(p, 24, 4, 0.5, seed + 5u);     // croûte sèche plus claire

  // Grain motteux : bruit « turbulent » (|bruit|) → sillons fins et irréguliers.
  float crumb = 1.5 * billowR(r, 67, 2, 0.5, seed + 6u);
  soilH += 0.0007 * crumb;

  float3 earth = lerp(float3(0.205, 0.14, 0.078), float3(0.195, 0.14, 0.086),
                      smoothstep(-0.3, 0.3, hue));
  earth *= (1.0 + 0.18 * mottle) * (1.0 + 0.25 * grain);
  earth *= 0.75 + 0.35 * crumb;
  earth *= (1.0 + 0.55 * (clods - 0.3)) * (1.0 + 0.8 * (rough - 0.7));
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
  float moss = 0.8 * smoothstep(0.47, 0.55, clump);
  float3 mossColor = lerp(float3(0.09, 0.11, 0.03), float3(0.16, 0.20, 0.05),
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
  float dust = 0.40 + big + 0.25 * saturate(dot(top.local, top.local)) + 0.2 * crust;
  rock = lerp(rock, earth, saturate(dust));
  albedo = lerp(albedo, rock, top.cover);
  height = max(height, soilH + top.height);

  Surface s = { height, saturate(albedo) };
  return s;
}
