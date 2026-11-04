// Briques de roche : réseau de fissures ramifiées (roche sombre, grès).
#pragma once
#include "procgen/lib/texgen.slang"
#include "lib/scale.slang"
#include "lib/detail.slang"

// Une famille de fissures : ligne de niveau zéro d'un bruit vu par une matrice ENTIÈRE
// (périodique même si elle étire : direction préférée des fissures).
struct CrackSet {
  int period;
  int2 row0, row1;
  uint seed;
};

float crackNoise(float2 p, CrackSet set) {
  float2 q = float2(dot(float2(set.row0), p), dot(float2(set.row1), p));
  return gradientNoiseP(frac(q), set.period, set.seed);
}

// Distance signée à la fissure, en unités de tuile : n/|∇n|, sinon la largeur gonfle là
// où le bruit varie lentement. Gradient lu sans `jag` : stable malgré le contour haché.
float crackDistance(float2 smoothP, float2 jag, CrackSet set) {
  const float e = 1.0 / 1024.0;
  float2 dx = float2(e, 0.0), dy = float2(0.0, e);
  float2 g = float2(crackNoise(smoothP + dx, set) - crackNoise(smoothP - dx, set),
                    crackNoise(smoothP + dy, set) - crackNoise(smoothP - dy, set)) / (2.0 * e);
  // Plancher : borne les bavures autour des points où le gradient s'annule.
  return crackNoise(smoothP + jag, set) / max(length(g), float(set.period));
}

struct FractureStyle {
  int period;         // fissures principales, sur la direction la moins étirée
  int2 row0, row1;    // matrice d'étirement
  float bend;         // sinuosité à grande échelle (cm)
  float jag;          // zigzag fin du contour (cm)
  float width;        // demi-largeur des fissures principales (cm) ; ÷2 puis ÷4 ensuite
  float depth;        // creusement des fissures principales (cm) ; ÷2 puis ÷5 ensuite
  float valley;       // largeur de la vallée autour d'une fissure principale (cm)
  float scarp;        // décalage de hauteur entre les deux lèvres (cm)
};

struct Fracture {
  float height;  // unités de tuile : vallées + escarpements
  float main;    // 0..1, cœur des fissures principales
  float branch;  // 0..1, branches
  float hair;    // 0..1, fissures capillaires
  float near;    // 0..1, voisinage des fissures principales (liseré minéral)
  float tint;    // 0..1, aléa du bloc entre fissures
};

// Hiérarchie : chaque côté d'une fissure a sa propre famille de branches, qui s'arrêtent
// donc sur elle (jonctions en T) ; idem des capillaires sur les branches. Largeur effilée
// par un masque : les fissures se terminent en pointe au lieu de s'interrompre.
Fracture fracture(float2 p, FractureStyle st, uint seed) {
  float2 bend = cm(st.bend) * float2(fbmR(p, 2, 3, 0.5, seed + 1u),
                                     fbmR(p, 2, 3, 0.5, seed + 2u));
  float2 jag = cm(st.jag) * float2(fbmR(p, 11, 5, 0.65, seed + 3u),
                                   fbmR(p, 11, 5, 0.65, seed + 4u));
  float2 q = p + bend;

  float gate1 = smoothstep(-0.35, -0.05, fbmR(p, 3, 3, 0.5, seed + 5u));
  float gate2 = smoothstep(-0.20, 0.15, fbmR(p, 5, 3, 0.5, seed + 6u));
  float gate3 = smoothstep(-0.15, 0.20, fbmR(p, 9, 3, 0.5, seed + 7u));
  float along = 0.5 + 0.5 * fbmR(p, 12, 3, 0.5, seed + 8u); // largeur variable

  CrackSet set1 = { st.period, st.row0, st.row1, seed + 10u };
  float s1 = crackDistance(q, jag, set1);
  uint side1 = s1 > 0.0 ? 1u : 0u;

  CrackSet set2 = { 2 * st.period + 1, st.row0, st.row1, seed + 20u + 7u * side1 };
  float s2 = crackDistance(q, jag, set2);
  uint side2 = s2 > 0.0 ? 1u : 0u;

  CrackSet set3 = { 5 * st.period + 2, st.row0, st.row1,
                    seed + 40u + 7u * side1 + 3u * side2 };
  float s3 = crackDistance(q, jag, set3);

  float d1 = abs(s1), d2 = abs(s2), d3 = abs(s3);
  float w1 = max(cm(st.width) * gate1 * (0.5 + 1.2 * along), 1e-5);
  float w2 = max(cm(0.6 * st.width) * gate2 * (0.5 + 1.2 * along), 1e-5);
  float w3 = max(cm(0.4 * st.width) * gate3, 1e-5);

  Fracture f;
  f.main = smoothstep(w1, 0.4 * w1, d1) * smoothstep(0.0, 0.15, gate1);
  f.branch = smoothstep(w2, 0.4 * w2, d2) * smoothstep(0.0, 0.15, gate2);
  f.hair = smoothstep(w3, 0.4 * w3, d3) * smoothstep(0.0, 0.2, gate3);
  f.near = gate1 * exp(-d1 / cm(0.5 * st.valley));
  f.tint = hash33(uint3(side1, side2, seed + 60u)).x;

  // Effet des branches annulé au ras de leur fissure mère : leur champ y est discontinu.
  float fade1 = smoothstep(0.0, cm(25.0), d1);
  float fade2 = smoothstep(0.0, cm(12.0), d2);

  float h = cm(st.scarp) * gate1 * smoothstep(-cm(5.0), cm(5.0), s1);
  h += cm(0.35 * st.scarp) * gate2 * fade1 * smoothstep(-cm(3.0), cm(3.0), s2);
  h -= cm(st.depth) * gate1 * exp(-d1 / cm(st.valley));
  h -= cm(0.5 * st.depth) * gate2 * fade1 * exp(-d2 / cm(0.4 * st.valley));
  h -= cm(0.2 * st.depth) * gate3 * fade1 * fade2 * exp(-d3 / cm(0.15 * st.valley));
  f.height = h;
  return f;
}
