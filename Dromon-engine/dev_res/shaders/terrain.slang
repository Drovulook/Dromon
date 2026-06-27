// Shader du terrain volumétrique (géométrie issue de marching cubes).

#include "common.slang"

// Entrées vertex : doivent matcher `TerrainVertex` côté Rust (locations 0,1,2).
struct VSInput {
  float3 inPosition;        // location 0
  float3 inNormal;          // location 1 — gradient de densité
  float4 inMaterialWeights; // location 2 — poids des 4 matériaux dominants
}

struct VSOutput {
  float4 pos : SV_Position;
  float3 normal;
  float4 materialWeights;
  float4 shadowCoord;
}

[shader("vertex")]
VSOutput terrainVertMain(VSInput input) {
  VSOutput output;
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  output.pos = mul(ubo.proj, mul(ubo.view, worldPos));
  output.normal = mul((float3x3)pc.normalMatrix, input.inNormal);
  output.materialWeights = input.inMaterialWeights;
  output.shadowCoord = mul(ubo.lightViewProj, worldPos);
  return output;
}

// Vertex shader « depth-only » de la passe d'ombre. Entrée minimale (position
// seule) → la pipeline d'ombre du terrain n'exige que la location 0.
struct ShadowVSInput {
  float3 inPosition; // location 0
}
[shader("vertex")]
float4 terrainShadowVertMain(ShadowVSInput input) : SV_Position {
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  return mul(ubo.lightViewProj, worldPos);
}

[shader("fragment")]
float4 terrainFragMain(VSOutput vertIn) : SV_Target {
  // Couleur de base MINIMALE : dégradé « terre → neige » piloté par le poids du
  // 1er matériau. Placeholder en attendant le vrai texture splatting (top-K).
  float3 dirt = float3(0.40, 0.30, 0.20);
  float3 snow = float3(0.90, 0.90, 0.92);
  float3 baseColor = lerp(dirt, snow, saturate(vertIn.materialWeights.x));

  float3 N = normalize(vertIn.normal);
  float3 L = normalize(-ubo.lightDirection.xyz);
  float NdotL = max(dot(N, L), 0.0);

  float shadow = computeShadow(vertIn.shadowCoord, NdotL);
  float3 lightCol = ubo.lightColor.rgb * ubo.lightColor.w;
  float3 ambient = 0.05 * baseColor;
  float3 color = baseColor * lightCol * NdotL * shadow + ambient;

  return float4(color, 1.0);
}
