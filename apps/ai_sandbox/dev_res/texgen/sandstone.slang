// Recette : grès ocre-rouge érodé, ISOTROPE : feuillets courbes (courbes de niveau d'un
// bruit), alvéoles, fissures discrètes, grain sableux. Pas de lits droits : répétés
// avec la tuile, ils tissent une grille visible de loin.
// Contrat : procgen/lib/texgen.slang.
#include "procgen/texgen.slang"
#include "lib/rock.slang"

Surface surface(float2 p, uint seed) {
  // Matrice de rotation (sans étirement) : fissures sans direction préférée.
  FractureStyle style = {
    2, int2(2, 1), int2(-1, 2),
    40.0,  // bend
    26.0,  // jag
    1.3,   // width
    7.0,   // depth
    18.0,  // valley
    3.0,   // scarp
  };
  Fracture f = fracture(p, style, seed);

  float2 q = p + cm(30.0) * float2(fbmR(p, 5, 4, 0.55, seed + 100u),
                                   fbmR(p, 5, 4, 0.55, seed + 101u));

  // --- Feuillets : gradins le long de courbes fermées, espacement irrégulier ---
  float sheetField = 7.0 * fbmR(q, 3, 4, 0.5, seed + 102u) + 20.0;
  float sheets = ledges(sheetField, 0.22);

  // --- Relief ---
  float billow = billowR(q, 6, 4, 0.5, seed + 103u);    // bosses et plis, ~1,5 m
  float rough = ridgedR(p, 34, 5, 0.6, seed + 104u);    // rugosité, 30 cm → 1,2 cm
  float grit = gradientNoiseP(p, 400, seed + 105u);     // grain, 2,5 cm
  // Alvéoles : petits creux arrondis à bord net, regroupés par zones.
  float zone = smoothstep(-0.1, 0.25, fbmR(p, 7, 3, 0.5, seed + 106u));
  float pits = smoothstep(0.12, 0.20, fbmR(q, 37, 3, 0.5, seed + 107u)) * zone;

  float height = f.height + cm(2.8) * sheets + cm(14.0) * billow + cm(2.6) * rough
                 + cm(0.3) * grit - cm(2.5) * pits;

  // --- Couleur : contrastes faibles aux grandes échelles (répétition de la tuile) ---
  float3 rust = float3(0.33, 0.20, 0.115);
  float3 pale = float3(0.43, 0.31, 0.19);
  // Teinte qui suit les feuillets, sans se répéter à l'identique d'un gradin au suivant.
  float band = 0.5 + 0.5 * sin(2.4 * sheetField + 2.0 * billow);
  float3 albedo = lerp(rust, pale, 0.35 + 0.28 * band);
  albedo *= 0.95 + 0.10 * f.tint;
  float varnish = fbmR(q, 31, 5, 0.6, seed + 110u) + 0.25 * (rough - 0.7);
  albedo = lerp(albedo, albedo * float3(0.84, 0.80, 0.78), smoothstep(0.08, 0.16, varnish));
  albedo = lerp(albedo, albedo * float3(0.88, 0.80, 0.74), 0.7 * pits); // sable oxydé
  albedo *= 1.0 + 1.5 * (rough - 0.7); // le détail net vient d'ici
  albedo *= 1.0 + 0.3 * grit;

  float3 washed = float3(0.42, 0.31, 0.21) * (0.7 + 0.6 * rough);
  albedo = lerp(albedo, washed, 0.35 * f.main + 0.25 * f.branch + 0.15 * f.hair);

  Surface s = { height, saturate(albedo) };
  return s;
}
