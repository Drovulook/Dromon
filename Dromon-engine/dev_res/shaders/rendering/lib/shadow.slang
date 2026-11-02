// Ombres portées : shadow map (set 2) et son échantillonnage filtré.
#pragma once
#include "bindings.slang"

// set 2, binding 0 : la shadow map. Sampler2DShadow = sampler de comparaison :
// SampleCmp(uv, ref) renvoie ~1.0 si ref <= profondeur stockée (fragment éclairé),
// ~0.0 sinon (fragment dans l'ombre), avec PCF 2×2 si filtrage linéaire.
[[vk::binding(0, 2)]]
Sampler2DShadow shadowMap;

// Réglages (render.ron → shadow, debug.ron → shadows).
float shadowMapTexelUv() { return ubo.shadowParams.z; } // 1 / résolution réelle
bool shadowsEnabled() { return ubo.shadowParams.w > 0.5; }
// Anti-acné, en texels de shadow map (valables quelle que soit la boîte d'ombre).
float normalOffsetTexels() { return ubo.shadowBias.x; }
float depthBiasTexels() { return ubo.shadowBias.y; }

// Renvoie un facteur d'éclairement ∈ [0,1] : 1 = pleinement éclairé, 0 = ombré.
// `N` normalisée, `NdotL` = max(dot(N, L), 0).
float computeShadow(float3 worldPos, float3 N, float NdotL) {
  if (!shadowsEnabled()) {
    return 1.0;
  }

  // Normal offset : on échantillonne depuis un point décollé de la surface le long
  // de N, pour qu'elle ne se compare plus à sa propre profondeur discrétisée.
  // Décalage ∝ sin(angle N/L) : nul face à la lumière, maximal en rasant.
  float texel = ubo.shadowParams.y;
  float sinNL = sqrt(1.0 - NdotL * NdotL);
  float3 samplePos = worldPos + N * (texel * normalOffsetTexels() * sinNL);
  float4 shadowCoord = mul(ubo.lightViewProj, float4(samplePos, 1.0));

  // Division perspective (triviale en orthographique, où w = 1).
  float3 proj = shadowCoord.xyz / shadowCoord.w;

  // En dehors du frustum de la lumière (au-delà du far) → considéré éclairé.
  if (proj.z > 1.0) {
    return 1.0;
  }

  // NDC xy ∈ [-1,1] → coordonnées de texture uv ∈ [0,1].
  float2 uv = proj.xy * 0.5 + 0.5;

  // Petit biais de profondeur constant (la pente est gérée par le normal offset),
  // unités monde → profondeur normalisée via l'épaisseur de la boîte.
  float bias = texel * depthBiasTexels() / ubo.shadowParams.x;

  // PCF 3×3 : on moyenne 9 texels voisins. Chaque tap est déjà filtré 2×2 par le
  // hardware (sampler LINEAR) → bord lissé sans escalier.
  float sum = 0.0;
  float texelUv = shadowMapTexelUv();
  for (int x = -1; x <= 1; x++) {
    for (int y = -1; y <= 1; y++) {
      float2 offset = float2(x, y) * texelUv;
      // LevelZero : la shadow map n'a qu'un niveau ; sans dérivées implicites, la
      // lecture reste valide dans une branche (terrain sous la mer).
      // résultat identique à SampleCmp
      sum += shadowMap.SampleCmpLevelZero(uv + offset, proj.z - bias);
    }
  }
  return sum / 9; // moyenne des 9 échantillons
}
