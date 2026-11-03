// Repli périodique des cellules, sûr pour les indices négatifs.
// ⚠ `wrapCell` (moteur) donne une couture sur les cellules −1 : `worleyP` n'est pas
// périodique (vérifié avec la recette seamtest). Utiliser `wrapCellSafe` / `worleyPSafe`.
#pragma once
#include "procgen/lib/texgen.slang"

uint2 wrapCellSafe(int2 cell, int period) {
  // Décalage positif (les voisins vont jusqu'à −2) : aucun `%` sur un négatif.
  return uint2(cell + 8 * period) % uint(period);
}

// Copie de `worleyP` avec `wrapCellSafe`.
Cell worleyPSafe(float2 p, int period, float jitter, uint seed) {
  float2 x = p * float(period);
  int2 i = int2(floor(x));
  float2 f = x - floor(x);
  Cell r = { 8.0, 8.0, float3(0.0) };
  for (int y = -1; y <= 1; y++) {
    for (int xo = -1; xo <= 1; xo++) {
      int2 o = int2(xo, y);
      uint2 c = wrapCellSafe(i + o, period);
      float3 h = hash33(uint3(c, seed));
      float2 point = float2(o) + 0.5 + (h.xy - 0.5) * jitter;
      float d = length(point - f);
      if (d < r.f1) {
        r.f2 = r.f1;
        r.f1 = d;
        r.id = hash33(uint3(c, seed + 0x9E37u));
      } else if (d < r.f2) {
        r.f2 = d;
      }
    }
  }
  return r;
}
