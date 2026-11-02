// Recette : terre sèche parsemée de petits cailloux. Contrat : procgen/lib/texgen.slang.
#include "procgen/texgen.slang"

Surface surface(float2 p, uint seed) {
  // Sol : ondulations douces (~1/6 de tuile) + grain fin.
  float soil = 0.004 * fbmP(p, 6, 5, seed);

  // Cailloux : un par cellule (14 par côté), dans une cellule sur deux. Dôme de rayon
  // 0.3 cellule, hauteur 0.004 tuile × variation par caillou.
  Cell c = worleyP(p, 14, 0.7, seed + 100u);
  float present = step(c.id.x, 0.5);
  float r = 0.3 * (0.6 + 0.6 * c.id.y);
  float dome = sqrt(saturate(1.0 - (c.f1 / r) * (c.f1 / r)));
  float pebble = present * dome;
  float height = soil + 0.004 * (0.5 + c.id.z) * pebble;

  // Couleur : brun variant à grande échelle, plus sombre dans les creux du sol ;
  // cailloux gris, teinte propre à chacun.
  float patches = fbmP(p, 3, 3, seed + 200u);
  float3 earth = float3(0.20, 0.14, 0.09) * (1.0 + 0.4 * patches) * (1.0 + 60.0 * soil);
  float3 stone = float3(0.32, 0.30, 0.27) * (0.7 + 0.6 * c.id.z);
  float3 albedo = lerp(earth, stone, smoothstep(0.0, 0.3, pebble));

  Surface s = { height, saturate(albedo) };
  return s;
}
