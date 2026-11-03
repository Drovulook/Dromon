// Recette : herbe dense et isotrope — brins verts de plusieurs teintes sur une canopée
// sombre, chaume sec par endroits, rares trèfles. Contrat : procgen/lib/texgen.slang.
#include "procgen/texgen.slang"
#include "lib/grass.slang"
#include "lib/ground.slang"

Surface surface(float2 p, uint seed) {
  // Variations douces : zones plus jaunes / plus vertes, plus ou moins denses.
  // Pas de grandes taches (répétition visible de loin) : périodes ≥ 8.
  float hue = fbmP(p, 8, 4, seed);
  float lush = fbmP(p, 10, 3, seed + 1u);

  // --- Fond : canopée sombre (herbe profonde), terre dans de rares trous ---
  float under = fbmP(p, 48, 4, seed + 2u);
  float3 soil = float3(0.05, 0.045, 0.022);
  float3 canopy = float3(0.04, 0.085, 0.015) * (1.0 + 0.5 * under);
  float3 albedo = lerp(soil, canopy, smoothstep(-0.8, -0.55, under + 0.2 * lush));
  float height = 0.0004 * under;

  // --- Chaume : brins secs couchés, sous le vert ; brun-olive, peu contrasté ---
  Blade thatch = scatterBlades(p, 80, 2, 0.6, 0.6, 0.18, 0.001, seed + 10u);
  float3 dry = float3(0.11, 0.115, 0.045) * (0.7 + 0.5 * thatch.id.x);
  albedo = lerp(albedo, dry, thatch.cover);
  height = max(height, thatch.height);

  // --- Brins verts : deux couches de 3 brins par cellule ---
  // Relief modéré : trop haut, l'AO cerne chaque brin de noir.
  Blade g = scatterBlades(p, 110, 3, 0.8 + 0.15 * lush, 0.6, 0.2, 0.0018, seed + 20u);
  Blade g2 = scatterBlades(p, 135, 3, 0.8 + 0.15 * lush, 0.6, 0.2, 0.0018, seed + 30u);
  if (g2.height > g.height) g = g2;
  // Brins posés au-dessus du chaume (décalage de hauteur).
  float gh = g.height > 0.0 ? 0.0004 + g.height : 0.0;

  float3 deep = float3(0.05, 0.11, 0.018);
  float3 bright = float3(0.15, 0.28, 0.04);
  float3 yellowish = float3(0.22, 0.27, 0.06);
  float3 blade = lerp(deep, bright, g.id.x);
  blade = lerp(blade, yellowish, 0.6 * smoothstep(0.8, 1.0, g.id.y)
                                 + 0.15 * smoothstep(0.0, 0.4, hue));
  // Pointe un peu plus claire (tissu plus fin), pas un éclairage.
  blade *= 1.0 + 0.2 * g.t * g.t;
  // Jeunes pousses vert vif et quelques brins secs parmi les verts.
  if (g.id.z < 0.08) blade = float3(0.20, 0.38, 0.045) * (1.0 + 0.2 * g.t);
  if (g.id.z > 0.97) blade = float3(0.20, 0.18, 0.09);

  if (gh > height) {
    albedo = lerp(albedo, blade, g.cover);
    height = gh;
  }

  // --- Trèfles : petites feuilles rondes, rares, par grappes ---
  float cloverZone = smoothstep(0.28, 0.42, fbmP(p, 12, 3, seed + 50u));
  Stones leaf = scatterStones(p, 120, 0.6 * cloverZone, float2(0.35, 0.5), 0.5, 0.15,
                              seed + 60u);
  float lh = leaf.height > 0.0 ? 0.0022 + leaf.height : 0.0;
  if (lh > height) {
    float3 cl = float3(0.09, 0.17, 0.035) * (0.8 + 0.4 * leaf.id.y);
    albedo = lerp(albedo, cl, leaf.cover);
    height = lh;
  }

  Surface s = { height, saturate(albedo) };
  return s;
}
