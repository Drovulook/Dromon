// Briques de couverts végétaux : brins d'herbe couchés.
#pragma once
#include "procgen/lib/texgen.slang"
#include "lib/wrap.slang"

// Brin visible en un point (le plus haut des brins qui le couvrent).
struct Blade {
  float height; // unités de tuile, 0 hors brin
  float cover;  // couverture 0..1, bords adoucis
  float3 id;    // aléas propres au brin (teinte…)
  float t;      // position le long du brin : 0 base → 1 pointe
  float across; // 0 sur la nervure → 1 au bord du brin
};

// `count` brins par cellule (`period` par côté), chacun avec probabilité `density` :
// segment courbe, effilé vers la pointe, direction aléatoire (isotrope). len :
// demi-longueur max en cellules (≤ 0.6, cf. parcours 3×3) ; width : demi-largeur à la
// base / demi-longueur. lift : hauteur max (unités de tuile) ; la pointe est plus haute.
Blade scatterBlades(float2 p, int period, int count, float density, float len,
                    float width, float lift, uint seed) {
  float2 x = p * float(period);
  int2 i = int2(floor(x));
  float2 f = x - floor(x);
  Blade b = { 0.0, 0.0, float3(0.0), 0.0, 0.0 };
  for (int y = -1; y <= 1; y++) {
    for (int xo = -1; xo <= 1; xo++) {
     for (int k = 0; k < count; k++) {
      int2 o = int2(xo, y);
      uint2 c = wrapCellSafe(i + o, period);
      uint sk = seed + uint(k) * 0x3C6Fu;
      float3 h = hash33(uint3(c, sk));
      if (h.z > density) continue;
      float3 id = hash33(uint3(c, sk + 0x9E37u));
      float3 sh = hash33(uint3(c, sk + 0x51EDu));
      float2 d = f - (float2(o) + 0.5 + (h.xy - 0.5) * 0.9);
      float a = sh.y * 6.2831853;
      float ca = cos(a), sa = sin(a);
      float2 q = float2(ca * d.x + sa * d.y, -sa * d.x + ca * d.y); // x le long du brin
      float L = len * (0.55 + 0.45 * sh.x);
      float u = q.x / L; // −1 base … +1 pointe
      if (abs(u) >= 1.0) continue;
      float t = 0.5 * u + 0.5;
      // Courbure : la ligne médiane s'écarte en parabole.
      float centre = (sh.z - 0.5) * 0.7 * L * u * u;
      float halfW = width * L * (1.0 - 0.9 * t);
      float s = abs(q.y - centre) / halfW;
      if (s >= 1.0) continue;
      // Section arrondie ; le brin se relève vers la pointe ; hauteur propre au brin.
      float height = lift * (0.35 * id.z + 0.45 * t + 0.2 * sqrt(1.0 - s * s));
      if (height > b.height) {
        b.height = height;
        b.cover = smoothstep(1.0, 0.5, s) * smoothstep(1.0, 0.85, abs(u));
        b.id = id;
        b.t = t;
        b.across = s;
      }
     }
    }
  }
  return b;
}
