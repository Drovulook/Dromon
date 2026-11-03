// Briques de sols : semis de pierres irrégulières (cailloux, gravier, grains).
#pragma once
#include "procgen/lib/texgen.slang"
#include "lib/wrap.slang"

// Pierre visible en un point (la plus haute des pierres qui le couvrent).
struct Stones {
  float height; // unités de tuile, 0 hors pierre
  float cover;  // couverture 0..1, bord adouci (pour fondre la couleur)
  float3 id;    // aléas propres à la pierre (palette, teinte…)
  float2 local; // position dans la pierre, ≈ disque unité
};

// Une pierre par cellule (`period` cellules par côté) avec probabilité `density`.
// radius : rayon min/max en cellules (≤ 0.6 : la pierre ne dépasse pas les 3×3 voisines).
// relief : hauteur / rayon. irregular : amplitude du contour bruité.
// Les pierres peuvent déborder de leur cellule → parcours des 3×3 voisines, max des hauteurs.
Stones scatterStones(float2 p, int period, float density, float2 radius, float relief,
                     float irregular, uint seed) {
  float2 x = p * float(period);
  int2 i = int2(floor(x));
  float2 f = x - floor(x);
  // Contour irrégulier : bruit lu dans l'espace de la tuile (périodique), commun aux pierres.
  float wobble = irregular * (0.7 * gradientNoiseP(p, period * 3, seed + 11u)
                              + 0.3 * gradientNoiseP(p, period * 7, seed + 12u));
  Stones s = { 0.0, 0.0, float3(0.0), float2(0.0) };
  for (int y = -1; y <= 1; y++) {
    for (int xo = -1; xo <= 1; xo++) {
      int2 o = int2(xo, y);
      uint2 c = wrapCellSafe(i + o, period);
      float3 h = hash33(uint3(c, seed));
      if (h.z > density) continue;
      float3 id = hash33(uint3(c, seed + 0x9E37u));
      float3 shape = hash33(uint3(c, seed + 0x51EDu));
      float2 d = f - (float2(o) + 0.5 + (h.xy - 0.5) * 0.8);
      // Plus de petites pierres que de grosses (tirage au carré).
      float r = lerp(radius.x, radius.y, shape.x * shape.x);
      float a = shape.y * 6.2831853;
      float ca = cos(a), sa = sin(a);
      float2 q = float2(ca * d.x + sa * d.y, -sa * d.x + ca * d.y)
                 / (r * float2(1.0, 0.55 + 0.45 * shape.z));
      float dist = length(q) + wobble;
      if (dist >= 1.0) continue;
      // Dôme aplati (pierre en partie enfouie), hauteur ∝ rayon.
      float profile = pow(saturate(1.0 - dist * dist), 0.6);
      float height = relief * (r / float(period)) * profile * (0.6 + 0.8 * id.z);
      if (height > s.height) {
        s.height = height;
        s.cover = smoothstep(1.0, 0.8, dist);
        s.id = id;
        s.local = q;
      }
    }
  }
  return s;
}

// Palette de pierres de sol (linéaire) : noir, gris, beige clair, ocre-rouge, brun.
float3 stoneColor(float3 id) {
  float3 c;
  if (id.x < 0.12)      c = float3(0.045, 0.042, 0.040);
  else if (id.x < 0.45) c = float3(0.20, 0.19, 0.175);
  else if (id.x < 0.70) c = float3(0.38, 0.33, 0.26);
  else if (id.x < 0.85) c = float3(0.30, 0.15, 0.085);
  else                  c = float3(0.26, 0.20, 0.13);
  return c * (0.7 + 0.6 * id.y);
}
