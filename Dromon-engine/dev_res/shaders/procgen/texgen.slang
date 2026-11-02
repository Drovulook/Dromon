// Harnais de texgen : points d'entrée compute autour d'une recette. La recette
// (`<app>/dev_res/texgen/<nom>.slang`) inclut ce fichier puis définit surface() (contrat :
// lib/texgen.slang) ; Slang résout les noms globaux sans ordre, d'où l'appel avant la
// définition. Le moteur compile la recette, un .spv par point d'entrée.
// Includes depuis dev_res/shaders/ (slangc -I).
// Passe 1 `surfaceMain` : hauteur + albédo. Passe 2 `normalAoMain` : normale + AO, dérivées
// de la hauteur. Un thread par texel ; les bords bouclent (tuile périodique).
#include "lib/texgen.slang"

// ⚠ Formats synchronisés avec les images créées par le moteur (texgen/gpu.rs).
[[vk::binding(0, 0)]] [format("r32f")] RWTexture2D<float> heightMap;
// 32 bits : relues par le CPU (pas de f16 natif en Rust).
[[vk::binding(1, 0)]] [format("rgba32f")] RWTexture2D<float4> albedoMap;    // rgb linéaire
[[vk::binding(2, 0)]] [format("rgba32f")] RWTexture2D<float4> normalAoMap;  // xyz NormalGL, w AO

struct TexgenPushConstants {
  uint size; // côté de l'image, en texels
  uint seed;
};
[[vk::push_constant]]
ConstantBuffer<TexgenPushConstants> texgen;

// AO de cavité : rayon du voisinage (unités de tuile) et gain sur la profondeur relative.
static const float AO_RADIUS = 0.01;
static const float AO_STRENGTH = 2.0;
static const int AO_RINGS = 3;
static const int AO_DIRECTIONS = 8;

[shader("compute")]
[numthreads(8, 8, 1)]
void surfaceMain(uint3 id: SV_DispatchThreadID) {
  if (any(id.xy >= texgen.size)) return;
  float2 p = (float2(id.xy) + 0.5) / float(texgen.size); // centre du texel
  Surface s = surface(p, texgen.seed);
  heightMap[id.xy] = s.height;
  albedoMap[id.xy] = float4(s.albedo, 1.0);
}

// Hauteur au texel `t`, coordonnées repliées dans la tuile.
float heightAt(int2 t) {
  int n = int(texgen.size);
  return heightMap[uint2(((t % n) + n) % n)];
}

[shader("compute")]
[numthreads(8, 8, 1)]
void normalAoMain(uint3 id: SV_DispatchThreadID) {
  if (any(id.xy >= texgen.size)) return;
  int2 t = int2(id.xy);
  float n = float(texgen.size);

  // Pentes par différences centrées, en unités de tuile : Δh sur 2 texels = 2/n.
  float dhdx = (heightAt(t + int2(1, 0)) - heightAt(t - int2(1, 0))) * n * 0.5;
  float dhdy = (heightAt(t + int2(0, 1)) - heightAt(t - int2(0, 1))) * n * 0.5;
  // NormalGL : y vers le HAUT de l'image, alors que les lignes croissent vers le bas →
  // signe de y inversé par rapport à x.
  float3 normal = normalize(float3(-dhdx, dhdy, 1.0));

  // Cavité = de combien le texel est sous la moyenne de son voisinage (cercles de
  // AO_RADIUS), rapportée au rayon → sans unité, indépendante de la résolution.
  float h = heightAt(t);
  float radius = AO_RADIUS * n;
  float sum = 0.0;
  for (int ring = 1; ring <= AO_RINGS; ring++) {
    float r = radius * float(ring) / float(AO_RINGS);
    for (int k = 0; k < AO_DIRECTIONS; k++) {
      // Directions décalées d'un cercle à l'autre : couvre mieux le disque.
      float a = 6.2831853 * (float(k) + 0.5 * float(ring)) / float(AO_DIRECTIONS);
      sum += heightAt(t + int2(round(r * float2(cos(a), sin(a)))));
    }
  }
  float cavity = max(0.0, sum / float(AO_RINGS * AO_DIRECTIONS) - h);
  float ao = saturate(1.0 - AO_STRENGTH * cavity / AO_RADIUS);

  normalAoMap[id.xy] = float4(normal, ao);
}
