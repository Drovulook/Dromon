// Briques de roche : plaques séparées par des fissures irrégulières.
#pragma once
#include "procgen/lib/texgen.slang"
#include "lib/wrap.slang"

struct Plates {
  float crack; // 0..1, 1 au fond d'une fissure
  float edge;  // f2 − f1 (cellules) : distance à la frontière la plus proche
  float3 id;   // aléas de la plaque
};

// Plaques de Worley (`period` par côté) aux frontières sinueuses (domain warp
// d'amplitude `warp` en unités de tuile, périodique). Fissure le long des frontières,
// demi-largeur `width` (cellules) fixe ; intensité modulée par un bruit → fissures qui
// s'interrompent (moduler la largeur ferait des bavures aux carrefours).
// presence : part des frontières fissurées, dans [0, 1].
Plates crackedPlates(float2 p, int period, float warp, float width, float presence,
                     uint seed) {
  float2 w = float2(fbmP(p, period * 2, 3, seed + 1u), fbmP(p, period * 2, 3, seed + 2u));
  Cell c = worleyPSafe(p + warp * w, period, 0.9, seed);
  float g = fbmP(p, period * 2, 2, seed + 3u);
  float t = 0.4 - 0.8 * presence; // seuil du bruit (≈ ±0.4 en pratique)
  float gate = smoothstep(t - 0.04, t + 0.04, g);
  Plates r;
  r.edge = c.f2 - c.f1;
  r.crack = gate * (1.0 - smoothstep(0.4 * width, width, r.edge)); // bord net
  r.id = c.id;
  return r;
}

// Dalle de roche en un point (cf. `terraces`).
struct Terrace {
  float height; // unités de tuile
  float rim;    // 0..1 : lèvre de la dalle, si elle domine sa voisine (bord usé)
  float edge;   // f2 − f1 (cellules) : distance à la frontière la plus proche
  float3 id;    // aléas de la dalle
};

// Gradins : une dalle par cellule de Worley (`period` par côté), frontières sinueuses
// (domain warp `warp`, unités de tuile). Chaque dalle à sa hauteur (`lift`) et bombée
// vers son point (`dome`) → une marche à chaque frontière, adoucie sur `ramp` cellules.
// Contrairement à des formes qui se chevauchent, aucune marche n'est cachée ni aucun
// trou laissé : chaque point appartient à une seule dalle.
Terrace terraces(float2 p, int period, float warp, float dome, float lift, float ramp,
                 uint seed) {
  float2 w = float2(fbmP(p, period * 2, 3, seed + 1u), fbmP(p, period * 2, 3, seed + 2u));
  float2 x = (p + warp * w) * float(period);
  int2 i = int2(floor(x));
  float2 f = x - floor(x);
  float f1 = 8.0, f2 = 8.0;
  float3 id1 = float3(0.5), id2 = float3(0.5);
  for (int y = -1; y <= 1; y++) {
    for (int xo = -1; xo <= 1; xo++) {
      int2 o = int2(xo, y);
      uint2 c = wrapCellSafe(i + o, period);
      float3 h = hash33(uint3(c, seed));
      float d = length(float2(o) + 0.5 + (h.xy - 0.5) * 0.9 - f);
      float3 id = hash33(uint3(c, seed + 0x9E37u));
      if (d < f1) {
        f2 = f1; id2 = id1;
        f1 = d; id1 = id;
      } else if (d < f2) {
        f2 = d; id2 = id;
      }
    }
  }
  float k = dome / float(period);
  float h1 = lift * id1.x - k * f1 * f1;
  float h2 = lift * id2.x - k * f2 * f2;
  // Sur la frontière (f1 = f2), les deux dalles valent leur moyenne → continu.
  float e = f2 - f1;
  Terrace t;
  t.height = lerp(0.5 * (h1 + h2), h1, smoothstep(0.0, ramp, e));
  t.rim = id1.x > id2.x ? 1.0 - smoothstep(0.0, 2.5 * ramp, e) : 0.0;
  t.edge = e;
  t.id = id1;
  return t;
}

// Bruit « ridged » périodique : somme de (1 − |bruit|)² → arêtes vives, creux arrondis.
// ≈ [0, 1].
// ⚠ Le bruit de gradient vaut 0 sur les nœuds de son réseau → pics en grille régulière,
// alignés d'une octave à l'autre. D'où : warp de période 2P+1 (réseau presque disjoint)
// et décalage fractionnaire propre à chaque octave (constant → reste périodique).
float ridgedP(float2 p, int period, int octaves, uint seed) {
  int q = 2 * period + 1;
  float2 w = float2(gradientNoiseP(p, q, seed + 50u), gradientNoiseP(p, q, seed + 51u));
  float2 pw = p + w * (0.4 / float(period));
  float sum = 0.0, amplitude = 0.5, norm = 0.0;
  for (int k = 0; k < octaves; k++) {
    float2 off = frac(float2(0.37, 0.71) * float(k + 1)) / float(period << k);
    float r = 1.0 - abs(gradientNoiseP(pw + off, period << k, seed + uint(k)));
    sum += amplitude * r * r;
    norm += amplitude;
    amplitude *= 0.5;
  }
  return sum / norm;
}
