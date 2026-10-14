// Hash procédural, sans texture.
#pragma once

// Hash entier pcg3d (Jarzynski & Olano, 2020) : 3 sorties décorrélées.
uint3 pcg3d(uint3 v) {
  v = v * 1664525u + 1013904223u;
  v.x += v.y * v.z; v.y += v.z * v.x; v.z += v.x * v.y;
  v ^= v >> 16u;
  v.x += v.y * v.z; v.y += v.z * v.x; v.z += v.x * v.y;
  return v;
}

float3 hash33(uint3 p) {
  return float3(pcg3d(p)) * (1.0 / 4294967295.0);
}

// Bruit de valeur 2D dans [0, 1] : hash aux nœuds entiers, interpolation lissée
// (smoothstep) entre eux. Coordonnées négatives : le cast int → uint garde les bits.
float valueNoise(float2 p) {
  int2 i = int2(floor(p));
  float2 f = frac(p);
  float2 u = f * f * (3.0 - 2.0 * f);
  float a = hash33(uint3(uint2(i), 0u)).x;
  float b = hash33(uint3(uint2(i + int2(1, 0)), 0u)).x;
  float c = hash33(uint3(uint2(i + int2(0, 1)), 0u)).x;
  float d = hash33(uint3(uint2(i + int2(1, 1)), 0u)).x;
  return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
}

// Bruit cellulaire (Worley) : distances F1, F2 aux deux points caractéristiques les plus
// proches, un point par cellule unité. Les points oscillent avec `t` (cellules vivantes).
// F2 − F1 ≈ 0 sur les frontières entre cellules.
float2 worley(float2 p, float t) {
  int2 i = int2(floor(p));
  float2 f = frac(p);
  float2 d = float2(8.0, 8.0);
  for (int y = -1; y <= 1; y++) {
    for (int x = -1; x <= 1; x++) {
      int2 o = int2(x, y);
      float3 h = hash33(uint3(uint2(i + o), 1u));
      float2 pt = float2(o) + 0.5 + 0.4 * sin(t + 6.2831853 * h.xy);
      float dist = length(pt - f);
      if (dist < d.x) {
        d.y = d.x;
        d.x = dist;
      } else if (dist < d.y) {
        d.y = dist;
      }
    }
  }
  return d;
}
