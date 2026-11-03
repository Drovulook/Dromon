// Recette : roche sombre en écailles qui se chevauchent (marches aux bords usés, liserés
// clairs), taches et veines fines. Inspirée de res/…/rock (photo), mais isotrope et sans
// motif plus grand que ~1/5 de tuile : rien ne trahit la répétition de loin.
// Contrat : procgen/lib/texgen.slang.
#include "procgen/texgen.slang"
#include "lib/rock.slang"

Surface surface(float2 p, uint seed) {
  // --- Relief : grandes écailles + lobes plus petits, veines fines ---
  // Marches hautes et nettes, rainure le long des joints : l'AO (calculée par le moteur
  // depuis la hauteur) assombrit les creux.
  Terrace big = terraces(p, 5, 0.05, 0.02, 0.025, 0.02, seed);
  Terrace lobe = terraces(p, 13, 0.02, 0.008, 0.008, 0.03, seed + 100u);
  Plates vein = crackedPlates(p, 11, 0.03, 0.02, 0.45, seed + 200u);

  float height = big.height + lobe.height;
  height -= 0.005 * (1.0 - smoothstep(0.0, 0.06, big.edge));
  height -= 0.002 * (1.0 - smoothstep(0.0, 0.08, lobe.edge));
  height -= 0.0015 * vein.crack;
  height += 0.0008 * fbmP(p, 48, 4, seed + 10u);           // surface irrégulière
  height += 0.0003 * gradientNoiseP(p, 240, seed + 11u);   // grain

  // --- Couleur ---
  // Contrastes modérés aux échelles moyennes : vus de loin, ils dessinent la grille de
  // répétition. Les détails fins (taches, grain) se fondent, eux.
  float3 base = float3(0.115, 0.105, 0.095) * (0.94 + 0.12 * big.id.z);
  base *= 0.94 + 0.12 * lobe.id.z;
  // Taches sombres (altération) et pâles (lichen gris), petites.
  float darkSpot = smoothstep(0.05, 0.25, fbmP(p, 32, 4, seed + 20u));
  base = lerp(base, base * 0.75, 0.6 * darkSpot);
  float paleSpot = smoothstep(0.3, 0.45, fbmP(p, 40, 3, seed + 21u));
  base = lerp(base, float3(0.16, 0.155, 0.15), 0.5 * paleSpot);
  base *= 1.0 + 0.15 * gradientNoiseP(p, 180, seed + 22u);

  // Liserés clairs ocre sur le bord usé des écailles (variation de matière : pierre
  // fraîchement cassée, pas un éclairage).
  // Liseré fin : de loin, il se fond dans la teinte moyenne au lieu de faire tache.
  float3 tan = float3(0.36, 0.26, 0.16);
  float3 albedo = lerp(base, tan, 0.5 * big.rim + 0.25 * lobe.rim);
  // Veines minérales claires.
  albedo = lerp(albedo, float3(0.22, 0.20, 0.18), 0.5 * vein.crack);

  Surface s = { height, saturate(albedo) };
  return s;
}
