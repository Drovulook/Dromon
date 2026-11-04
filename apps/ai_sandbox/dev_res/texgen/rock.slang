// Recette : roche sombre fracturée, inspirée de res/…/rock (photo). Bosses séparées de
// plis, écailles aux bords déchiquetés, fissures ramifiées à liseré minéral clair.
// Contrat : procgen/lib/texgen.slang. Tailles en cm : lib/scale.slang.
#include "procgen/texgen.slang"
#include "lib/rock.slang"

Surface surface(float2 p, uint seed) {
  // Fissures allongées selon y (fréquence ×2 en x), sinueuses, comme sur la photo.
  FractureStyle style = {
    2, int2(2, 0), int2(0, 1),
    45.0,  // bend
    26.0,  // jag
    1.6,   // width
    9.0,   // depth
    22.0,  // valley
    5.0,   // scarp
  };
  Fracture f = fracture(p, style, seed);

  // --- Relief : du mètre au centimètre ---
  // Warp fort : bosses aux contours irréguliers.
  float2 w = cm(35.0) * float2(fbmR(p, 5, 4, 0.55, seed + 100u),
                               fbmR(p, 5, 4, 0.55, seed + 101u));
  float2 wp = p + w;
  float lumps = fbmR(wp, 3, 3, 0.5, seed + 102u);             // masse, ~3 m
  float billow = billowR(wp, 6, 4, 0.5, seed + 103u);         // bosses et plis, ~1,5 m
  float scales = ledges(4.0 * fbmR(wp, 7, 6, 0.6, seed + 104u) + 8.0, 0.14); // écailles
  float rough = ridgedR(p, 28, 5, 0.6, seed + 105u);          // rugosité, 35 cm → 1,5 cm
  float grit = gradientNoiseP(p, 330, seed + 106u);           // grain, 3 cm

  float height = f.height + cm(16.0) * lumps + cm(14.0) * billow + cm(2.6) * scales
                 + cm(3.2) * rough + cm(0.35) * grit;

  // --- Couleur ---
  float3 albedo = float3(0.118, 0.106, 0.096) * (0.90 + 0.20 * f.tint);
  albedo *= 0.92 + 0.45 * (billow - 0.3);
  // Taches d'altération à bord net, fines, au contour haché par `rough`.
  // Petites (≲ 50 cm) : plus grandes, elles dessineraient la grille de répétition.
  float blotch = fbmR(wp, 19, 5, 0.6, seed + 110u) + 0.25 * (rough - 0.7);
  albedo = lerp(albedo, albedo * 0.80, smoothstep(0.05, 0.12, blotch));
  float lichen = fbmR(wp, 29, 5, 0.6, seed + 111u) + 0.3 * (rough - 0.7);
  albedo = lerp(albedo, float3(0.15, 0.145, 0.135), 0.4 * smoothstep(0.26, 0.31, lichen));
  // Grain minéral net : c'est lui qui se lit de près.
  albedo *= 1.0 + 1.1 * (rough - 0.7);
  albedo *= 1.0 + 0.18 * grit;

  // Calcite claire dans les fissures, halo très léger autour des principales.
  // Remplissage irrégulier : un trait de couleur uni ferait dessiné.
  // Discret : ×2 en jeu (albédo du matériau / moyenne), et un trait clair vu de loin
  // dessine la répétition de la tuile. La fissure se lit surtout par son relief.
  float3 calcite = float3(0.22, 0.175, 0.13) * (0.6 + 0.8 * saturate(rough + 0.3 * grit - 0.3));
  albedo = lerp(albedo, calcite, 0.7 * f.main);
  albedo = lerp(albedo, calcite, 0.6 * f.branch);
  albedo = lerp(albedo, calcite * 0.8, 0.5 * f.hair);

  Surface s = { height, saturate(albedo) };
  return s;
}
