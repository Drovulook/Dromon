// Recette : sable à rides éoliennes, grains sombres, rares débris clairs.
// Contrat : procgen/lib/texgen.slang.
#include "procgen/texgen.slang"
#include "lib/ground.slang"
#include "lib/detail.slang"
#include "lib/scale.slang"

// Ride asymétrique, 0..1 : `f` = phase ; montée douce, descente raide, sans arête.
float ripple(float f) {
  const float TAU = 6.2831853;
  return 0.5 + 0.5 * sin(TAU * (f + 0.18 * sin(TAU * f)));
}

Surface surface(float2 p, uint seed) {
  // --- Rides : ~17 cm, vecteur d'onde entier (périodique), ondulées et interrompues ---
  float bend = 3.0 * fbmR(p, 4, 3, 0.5, seed);
  float phase = dot(float2(9.0, 58.0), p) + bend;
  float presence = smoothstep(-0.25, 0.15, fbmR(p, 5, 3, 0.5, seed + 1u));
  float ripples = ripple(frac(phase)) * presence;
  // Rides plus fines par-dessus, dans une direction voisine.
  float fine = ripple(frac(dot(float2(-14.0, 139.0), p) + 2.0 * bend)) * (0.3 + 0.7 * presence);

  float swell = fbmR(p, 3, 3, 0.5, seed + 2u);          // ondulations, ~3 m
  float grain = gradientNoiseP(p, 800, seed + 3u);      // grain, ~1 cm
  float crumble = billowR(p, 60, 3, 0.55, seed + 4u);   // sable remué, 15 → 3 cm
  float height = cm(7.0) * swell + cm(0.8) * ripples + cm(0.22) * fine
                 + cm(0.35) * crumble + cm(0.08) * grain;

  // --- Couleur ---
  // Crêtes un peu plus claires (grains triés par le vent : matière, pas éclairage).
  float3 albedo = lerp(float3(0.54, 0.45, 0.30), float3(0.60, 0.51, 0.35), ripples);
  albedo *= 1.0 + 0.10 * fbmR(p, 13, 3, 0.5, seed + 5u);
  albedo *= 1.0 + 0.30 * (crumble - 0.3);
  albedo *= 1.0 + 0.14 * grain;

  // --- Grains colorés (~1–2 cm) : ceux qui se lisent de près ---
  Stones dark = scatterStones(p, 260, 0.30, float2(0.15, 0.35), 0.5, 0.1, seed + 10u);
  albedo = lerp(albedo, float3(0.30, 0.25, 0.18), 0.6 * dark.cover);
  height += dark.height;
  Stones warm = scatterStones(p, 210, 0.20, float2(0.15, 0.35), 0.5, 0.1, seed + 20u);
  albedo = lerp(albedo, float3(0.62, 0.36, 0.20), 0.6 * warm.cover);

  // --- Rares débris clairs (coquilles, galets, 3–6 cm) ---
  Stones shell = scatterStones(p, 70, 0.04, float2(0.12, 0.30), 0.35, 0.25, seed + 30u);
  float3 pale = float3(0.66, 0.60, 0.48) * (0.8 + 0.3 * shell.id.y);
  albedo = lerp(albedo, pale, 0.6 * shell.cover); // à moitié ensablés
  height = max(height, cm(7.0) * swell + cm(0.8) * ripples + shell.height);

  Surface s = { height, saturate(albedo) };
  return s;
}
